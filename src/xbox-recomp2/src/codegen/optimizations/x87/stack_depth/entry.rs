//! Finds `#stack` when each function starts.
//!
//! See "5. Find `#stack` at entry" in `x87.md`.

use super::super::Depth;
use super::super::call_target::{CallTarget, CallTargets};
use super::super::instruction::{DepthChange, is_uncountable, stack_effect};
use super::function::count_from;
use super::{FunctionAnalysis, OpaqueReason, StackCount, X87Effect, passes_extra_values};
use crate::analysis::{BruteForceKind, FunctionDiscoverySource, FunctionGraph};
use crate::codegen::function::model::FunctionModel;
use crate::codegen::optimizations::helpers::known_callers::found_only_by_code;
use crate::codegen::optimizations::helpers::model_index::ModelIndex;
use crate::codegen::optimizations::helpers::op_graph::{OpGraph, OpIndex};
use core::mem;
use xbox_recomp2_executable::GuestAddress32;

/// `#stack` at a function's entry, per its callers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryDepth {
    /// No caller seen yet.
    Unset,

    /// Every caller so far starts it with this many values.
    At(Depth),

    /// Callers disagree, or one's `#stack` is unknown.
    Unknown,
}

/// What steps 1 to 4 found, one item per function in model order.
pub(super) struct EntryFinder<'a, 'b> {
    pub graphs: &'a [Option<OpGraph<'b>>],
    pub call_targets: &'a CallTargets<'a>,
    pub effects: &'a [X87Effect],
    pub trusted: &'a [bool],
    pub may_read_extra_values: &'a [bool],
    pub called_by: &'a [CalledBy],
}

/// Who may call a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum CalledBy {
    /// Only the calls and jumps we walked.
    Known,

    /// Also code we can't see, like through a pointer.
    Unknown,

    /// Nothing we found: only the padding scan guessed it.
    /// Often a misread from the middle of another function.
    Guessed,
}

impl EntryFinder<'_, '_> {
    /// Pushes `(called function, #stack)` to `starts` for each call `caller` makes.
    /// `#stack` is [`None`] if unknown.
    ///
    /// Which calls we believe depends on the caller:
    ///
    /// - Sure of its count: every call.
    /// - Opaque or untrusted: calls after a reset, or with exactly the called
    ///   function's x87 arguments on the stack.
    /// - [`CalledBy::Guessed`]: only that exact match, and none if it's opaque.
    fn call_depths_from(
        &self,
        functions: &[FunctionAnalysis],
        caller: ModelIndex,
        entry: EntryDepth,
        starts: &mut Vec<(ModelIndex, Option<Depth>)>,
    ) {
        let function = &functions[caller];
        let opaque = function.opaque.is_some();
        let guessed = self.called_by[caller] == CalledBy::Guessed;
        if entry == EntryDepth::Unset || (guessed && opaque) {
            return;
        }

        let sure = !opaque && self.trusted[caller] && !guessed;
        let after_reset = match &self.graphs[caller] {
            Some(graph) if !sure && !guessed => self.ops_after_reset(graph, function),
            _ => None,
        };

        for call in &function.count.calls {
            let called = match call.target {
                CallTarget::Function(called) => called,
                CallTarget::Import | CallTarget::Indirect | CallTarget::Unknown => continue,
            };

            let reset_first = match &after_reset {
                Some(after_reset) => after_reset[call.op],
                None => false,
            };

            let args = functions[called].args;

            let depth = if reset_first {
                Some(call.depth)
            } else if entry == EntryDepth::Unknown {
                None
            } else if sure || call.depth == args {
                Some(call.depth)
            } else {
                None
            };

            starts.push((called, depth));
        }
    }

    /// Ops whose `#stack` we know from a reset alone, like a call after `fnsave`.
    ///
    /// ```asm
    /// fnsave [ecx+8]      ; stack now empty
    /// call   0x3826A5     ; starts at 0
    /// frstor [ecx+8]      ; can't count this, so `_CIpow` is opaque
    /// ```
    ///
    /// [`None`] if the counts are no good even after a reset.
    fn ops_after_reset(&self, graph: &OpGraph, function: &FunctionAnalysis) -> Option<Vec<bool>> {
        // Disagreeing paths or an untrusted called function. No good.
        if !matches!(function.opaque, None | Some(OpaqueReason::DepthUnknown)) {
            return None;
        }

        for call in &function.count.calls {
            if let CallTarget::Function(called) = call.target
                && !self.trusted[called]
            {
                return None;
            }
        }

        // Unsure from the entry, and after each move we can't count.
        let mut unsure = vec![false; graph.len()];
        let mut pending = vec![graph.entry];
        for op in 0..graph.len() {
            let paired = function.count.paired_frstors.contains(&op);
            if self.op_is_uncountable(graph, op) && !paired {
                pending.extend(graph.local_successors(op));
            }
        }

        // Still unsure along every path, up to the next reset.
        while let Some(op) = pending.pop() {
            let op = op as OpIndex;
            if mem::replace(&mut unsure[op], true) || op_empties_stack(graph, op) {
                continue;
            }

            pending.extend(graph.local_successors(op));
        }

        Some(unsure.into_iter().map(|unsure| !unsure).collect())
    }

    fn op_is_uncountable(&self, graph: &OpGraph, op: OpIndex) -> bool {
        let fast_trig = self.call_targets.hacks.fast_trig;
        let mut guests = graph.ops[op].guests();
        guests.any(|guest| is_uncountable(&guest.insn, fast_trig))
    }

    /// Updates function `index`'s entry with one caller's `depth`.
    /// Returns whether it changed.
    fn add_caller_depth(
        &self,
        functions: &mut [FunctionAnalysis],
        entries: &mut [EntryDepth],
        index: ModelIndex,
        depth: Option<Depth>,
    ) -> bool {
        let next = match (entries[index], depth) {
            // The first caller.
            (EntryDepth::Unset, Some(depth)) => {
                self.try_start_at(&mut functions[index], index, depth)
            }

            // Nothing new.
            (EntryDepth::At(at), Some(depth)) if at == depth => return false,
            (EntryDepth::Unknown, _) => return false,

            // Callers disagree, or this one doesn't know.
            _ => EntryDepth::Unknown,
        };

        entries[index] = next;
        true
    }

    /// Starts function `index` at `depth`, recounting it if its count starts elsewhere.
    fn try_start_at(
        &self,
        function: &mut FunctionAnalysis,
        index: ModelIndex,
        depth: Depth,
    ) -> EntryDepth {
        if depth == function.args && !function.hidden_args {
            return EntryDepth::At(depth);
        }

        let graph = match &self.graphs[index] {
            Some(graph) => graph,
            None => return EntryDepth::Unknown,
        };

        match self.try_recount_from(graph, function, self.effects[index], depth) {
            Some(count) => {
                function.count = count;
                EntryDepth::At(depth)
            }
            None => EntryDepth::Unknown,
        }
    }

    /// Recounts a function, starting with `depth` values.
    /// [`None`] unless the new count is as sure as the one from its arguments.
    fn try_recount_from(
        &self,
        graph: &OpGraph,
        function: &FunctionAnalysis,
        effect: X87Effect,
        depth: Depth,
    ) -> Option<StackCount> {
        let calls = &function.count.calls;
        if calls.iter().any(|call| call.target == CallTarget::Indirect) {
            return None;
        }

        // Every exit must leave what callers expect.
        let exit = depth.checked_sub(effect.stack_at_entry)? + effect.stack_at_exit;
        let count = count_from(graph, self.call_targets, self.effects, depth, 0);
        let sure = count.opaque.is_none()
            && count.missing_values.is_empty()
            && count.calls_missing_values.is_empty()
            && count.exits.iter().all(|&at| at == exit)
            && !passes_extra_values(&count, self.effects, self.may_read_extra_values);
        sure.then_some(count)
    }
}

/// Sets each function's [`FunctionAnalysis::entry`].
///
/// Functions that callers start above their x87 arguments get recounted from
/// there, like `_ftol` in `x87.md`.
pub(super) fn find_stack_at_entry(finder: &EntryFinder, functions: &mut [FunctionAnalysis]) {
    // Start from functions that code we can't see may call.
    // We assume it leaves just the x87 arguments on the stack.
    let mut entries = vec![EntryDepth::Unset; functions.len()];
    let mut pending = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let index = ModelIndex::from(index);
        if finder.called_by[index] == CalledBy::Known {
            continue;
        }

        entries[index] = match function.hidden_args {
            false => EntryDepth::At(function.args),
            true => EntryDepth::Unknown,
        };

        pending.push(index);
    }

    // Pass each caller's `#stack` on to the functions it calls, until nothing changes.
    let mut starts = Vec::new();
    while let Some(caller) = pending.pop() {
        starts.clear();
        finder.call_depths_from(functions, caller, entries[caller], &mut starts);

        for &(called, depth) in &starts {
            if finder.add_caller_depth(functions, &mut entries, called, depth) {
                pending.push(called);
            }
        }
    }

    // Functions no known code reaches stay unknown.
    for (function, entry) in functions.iter_mut().zip(entries) {
        function.entry = match entry {
            EntryDepth::At(depth) => Some(depth),
            EntryDepth::Unset | EntryDepth::Unknown => None,
        };
    }
}

/// Who may call each function, in model order.
pub(in super::super) fn find_who_may_call(
    models: &[FunctionModel],
    graph: &FunctionGraph,
) -> Vec<CalledBy> {
    let padding = FunctionDiscoverySource::BruteForce {
        kind: BruteForceKind::Padding,
    };

    let mut called_by = Vec::with_capacity(models.len());
    for model in models {
        let address = GuestAddress32::from(model.address.0);
        let incoming = graph.incoming(address);

        // Only the padding scan found it, and nothing refers to it.
        let only_padding = match graph.discovery_sources(address) {
            Some(sources) => sources.iter().all(|source| *source == padding),
            None => false,
        };

        if incoming.is_empty() && only_padding {
            called_by.push(CalledBy::Guessed);
            continue;
        }

        let through_pointer = incoming.iter().any(|reference| reference.kind.is_guess());
        if through_pointer || !found_only_by_code(address, graph, true) {
            called_by.push(CalledBy::Unknown);
            continue;
        }

        called_by.push(CalledBy::Known);
    }

    called_by
}

/// Whether op `op` empties the stack, like `fninit` or `fnsave`.
fn op_empties_stack(graph: &OpGraph, op: OpIndex) -> bool {
    let mut guests = graph.ops[op].guests();
    guests.any(|guest| stack_effect(&guest.insn).change == DepthChange::Reset)
}
