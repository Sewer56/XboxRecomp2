//! Counts `#stack` through one function.
//!
//! See steps 1 and 2 in `x87.md`.

use super::super::Depth;
use super::super::call_target::{CallTarget, CallTargets};
use super::super::instruction::{DepthChange, ffree_slot, is_uncountable, stack_effect};
use super::arguments::{X87Arguments, find_ops_before_any_call, find_x87_arguments};
use super::patterns::{
    ffree_run_empties_stack, find_fnsave_frstor_pairs, virtual_call_returns_float,
};
use super::{CallSite, FunctionAnalysis, MissingValues, OpaqueReason, StackCount, X87Effect};
use crate::codegen::function::model::Op;
use crate::codegen::optimizations::helpers::op_graph::{Edge, OpGraph, OpIndex};
use crate::decode::{Insn, Mnemonic};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// The most values the x87 stack holds.
const MAX_DEPTH: Depth = 8;

// ── 1. Count the stack in each function ──────────────────────────────────────

/// What every count of one function shares.
pub(super) struct StackCounter<'a, 'b> {
    graph: &'a OpGraph<'b>,
    call_targets: &'a CallTargets<'a>,
    effects: &'a [X87Effect],
    virtual_call_guess: Depth,
}

/// One count in progress.
struct Walker<'a, 'b> {
    graph: &'a OpGraph<'b>,
    call_targets: &'a CallTargets<'a>,
    effects: &'a [X87Effect],

    /// The `fnsave` op of each matched `frstor` op.
    fnsave_of: &'a BTreeMap<OpIndex, OpIndex>,

    virtual_call_guess: Depth,
    result: StackCount,

    /// Ops still to visit, each with `#stack` before it.
    pending: Vec<(OpIndex, Depth)>,
}

impl StackCounter<'_, '_> {
    /// Counts the function, starting with `args` values.
    pub(super) fn count_with_arguments(&self, args: Depth) -> StackCount {
        count_from(
            self.graph,
            self.call_targets,
            self.effects,
            args,
            self.virtual_call_guess,
        )
    }
}

impl Walker<'_, '_> {
    /// Steps through `op` from `depth`, and queues where it goes.
    fn visit_op(&mut self, op: OpIndex, mut depth: Depth) {
        let graph = self.graph;

        // A hook runs first, and may return or jump.
        if let Some(hook) = graph.hook(op) {
            if hook.may_return {
                self.result.exits.push(depth);
            }

            for &target in &hook.jumps {
                self.pending.push((target as OpIndex, depth));
            }
        }

        // Then each instruction in the op.
        let mut last_address = None;
        graph.for_each_instruction(op, |instruction| {
            let guest = instruction.guest;
            last_address = Some(guest.address);
            depth = self.depth_after_instruction(op, guest.address, &guest.insn, depth);
            if let Some(jump) = instruction.jump {
                self.follow_edge(op, guest.address, jump, depth);
            }
        });

        // Then how it leaves.
        match graph.ops[op] {
            Op::Call(call) => {
                let target = self.call_targets.find_call_target(call);
                self.follow_call(op, call.guest.address, target, depth)
            }
            Op::Return(_) => self.result.exits.push(depth),
            Op::Failed { .. } => self.result.opaque = Some(OpaqueReason::Unfollowable),
            // `int3` traps, so nothing after it runs.
            Op::Guest(guest) if guest.insn.mnemonic() == Mnemonic::INT3 => {}
            Op::Guest(_) | Op::CdqIdiv(_) | Op::MergedCmpJmp(_) => {
                let address = last_address.unwrap_or_default();
                for &edge in graph.end_edges(op) {
                    self.follow_edge(op, address, edge, depth);
                }
            }
        }
    }

    /// `#stack` after one instruction.
    fn depth_after_instruction(
        &mut self,
        op: OpIndex,
        address: GuestAddress32,
        insn: &Insn,
        depth: Depth,
    ) -> Depth {
        // A matched `frstor` brings back the `#stack` from before its `fnsave`.
        if insn.mnemonic() == Mnemonic::FRSTOR
            && let Some(&save) = self.fnsave_of.get(&op)
        {
            return self.depth_after_paired_frstor(op, save, depth);
        }

        // An `ffree` run that empties the stack counts as pops.
        let hacks = self.call_targets.hacks;
        if hacks.fast_ffree
            && depth > 0
            && ffree_slot(insn) == Some(depth - 1)
            && ffree_run_empties_stack(self.graph, op, depth - 1)
        {
            self.result.ffree_pops.insert(address);
            return depth - 1;
        }

        // Anything else follows the table in step 1 of `x87.md`.
        if is_uncountable(insn, hacks.fast_trig) {
            self.result.opaque = Some(OpaqueReason::DepthUnknown);
        }

        let effect = stack_effect(insn);
        let depth = self.note_missing_values(op, depth, effect.reads);
        match effect.change {
            DepthChange::Reset => 0,
            DepthChange::By(by) => self.clamp_depth(i16::from(depth) + i16::from(by)),
        }
    }

    /// `#stack` after the `frstor` at `op`: same as before the `fnsave` at `save`.
    fn depth_after_paired_frstor(&mut self, op: OpIndex, save: OpIndex, depth: Depth) -> Depth {
        match self.result.before[save] {
            Some(saved) => {
                self.result.paired_frstors.insert(op);
                saved
            }
            None => {
                self.result.opaque = Some(OpaqueReason::DepthUnknown);
                depth
            }
        }
    }

    /// Notes when `op` reads more than `depth` values, and counts on as if it had them.
    fn note_missing_values(&mut self, op: OpIndex, depth: Depth, reads: Depth) -> Depth {
        if reads <= depth {
            return depth;
        }

        let missing = reads - depth;
        self.result
            .missing_values
            .push(MissingValues { op, missing });
        reads
    }

    /// Clamps `depth` to 0 to 8, marking the count opaque if it's outside.
    fn clamp_depth(&mut self, depth: i16) -> Depth {
        if !(0..=i16::from(MAX_DEPTH)).contains(&depth) {
            self.result.opaque = Some(OpaqueReason::DepthUnknown);
        }

        depth.clamp(0, i16::from(MAX_DEPTH)) as Depth
    }
}

// ── 2. At each call, use the called function's count ─────────────────────────

impl Walker<'_, '_> {
    /// Steps over a call, and queues the ops after it.
    fn follow_call(
        &mut self,
        op: OpIndex,
        address: GuestAddress32,
        target: CallTarget,
        depth: Depth,
    ) {
        let returned_float = match target {
            CallTarget::Indirect => virtual_call_returns_float(self.graph, op, depth),
            _ => None,
        };

        let site = CallSite {
            op,
            address,
            target,
            depth,
        };

        let after = match self.apply_call_effect(site, returned_float) {
            Some(after) => after,
            None => return,
        };

        let after = self.clamp_depth(after);
        self.result.after_call.insert(op, after);
        for &edge in self.graph.end_edges(op) {
            self.follow_edge(op, address, edge, after);
        }
    }

    /// Follows one way out of `op`, at `depth`.
    fn follow_edge(&mut self, op: OpIndex, address: GuestAddress32, edge: Edge, depth: Depth) {
        let target = match edge {
            Edge::Op(target) => return self.pending.push((target as OpIndex, depth)),
            Edge::Abort => return,
            Edge::Unknown => {
                self.result.opaque = Some(OpaqueReason::Unfollowable);
                return;
            }
            Edge::Indirect => CallTarget::Indirect,
            Edge::Function(target) => self.call_targets.find_jump_target(target),
        };

        // A tail call.
        let site = CallSite {
            op,
            address,
            target,
            depth,
        };

        let exit = match self.apply_call_effect(site, None) {
            Some(exit) => exit,
            None => return,
        };

        if exit < 0 {
            self.result.opaque = Some(OpaqueReason::DepthUnknown);
            return;
        }

        let exit = self.clamp_depth(exit);
        self.result.exits.push(exit);
    }

    /// Records the call at `site`, and gives `#stack` once it's done.
    ///
    /// - Below 0 if it takes more values than there are.
    /// - [`None`] if we don't know the function.
    fn apply_call_effect(&mut self, site: CallSite, returned_float: Option<bool>) -> Option<i16> {
        let mut effect = match effect_of_calling(site.target, self.effects) {
            Some(effect) => effect,
            None => {
                self.result.opaque = Some(OpaqueReason::Unfollowable);
                return None;
            }
        };

        if site.target == CallTarget::Indirect {
            effect.stack_at_exit = match returned_float {
                Some(returns_float) => Depth::from(returns_float),
                None => {
                    self.result.guessed = true;
                    self.virtual_call_guess
                }
            };
        }

        // It takes more than we have: hidden x87 arguments.
        // Not counted, or a call loop would raise them forever.
        if effect.stack_at_entry > site.depth {
            self.result.calls_missing_values.push(site.op);
        }

        self.result.calls.push(site);

        let taken = i16::from(effect.stack_at_entry);
        let left = i16::from(effect.stack_at_exit);
        Some(i16::from(site.depth) - taken + left)
    }
}

/// Counts `#stack` through `graph`, starting with `start` values.
///
/// `virtual_call_guess` is what an unread virtual call leaves: 0 or 1.
pub(super) fn count_from(
    graph: &OpGraph,
    call_targets: &CallTargets,
    effects: &[X87Effect],
    start: Depth,
    virtual_call_guess: Depth,
) -> StackCount {
    let fnsave_of = find_fnsave_frstor_pairs(graph);
    let mut walker = Walker {
        graph,
        call_targets,
        effects,
        fnsave_of: &fnsave_of,
        virtual_call_guess,
        result: StackCount {
            before: vec![None; graph.len()],
            ..StackCount::default()
        },
        pending: vec![(graph.entry as OpIndex, start)],
    };

    while let Some((op, depth)) = walker.pending.pop() {
        // Paths must meet with the same `#stack`.
        match walker.result.before[op] {
            Some(seen) if seen == depth => continue,
            Some(_) => {
                walker.result.opaque = Some(OpaqueReason::DepthConflict);
                continue;
            }
            None => walker.result.before[op] = Some(depth),
        }

        walker.visit_op(op, depth);
    }

    walker.result
}

// ── When branches disagree ───────────────────────────────────────────────────

/// Counts one function, guessing what unread virtual calls returned.
pub(super) fn count_function(
    graph: &OpGraph,
    call_targets: &CallTargets,
    effects: &[X87Effect],
) -> FunctionAnalysis {
    // First guess: they returned nothing.
    let without_float = count_with_guess(graph, call_targets, effects, 0);
    let paths_disagree = matches!(
        without_float.opaque,
        Some(
            OpaqueReason::DepthConflict
                | OpaqueReason::ExitConflict
                | OpaqueReason::MissingValuesAfterCall
        )
    );
    if !without_float.count.guessed || !paths_disagree {
        return without_float;
    }

    // Second guess: they all returned a float. Keep it if the paths agree.
    let with_float = count_with_guess(graph, call_targets, effects, 1);
    match with_float.opaque {
        None => with_float,
        Some(_) => without_float,
    }
}

/// What calling `target` does to the x87 stack. [`None`] if we don't know it.
pub(super) fn effect_of_calling(target: CallTarget, effects: &[X87Effect]) -> Option<X87Effect> {
    match target {
        CallTarget::Function(index) => Some(effects[index]),
        CallTarget::Import => Some(X87Effect::NONE),
        // Virtual calls look ahead instead; see `virtual_call_returns_float`.
        CallTarget::Indirect => Some(X87Effect::NONE),
        CallTarget::Unknown => None,
    }
}

/// Whether `count` exits with different `#stack`, or with more than one value.
pub(super) fn exits_conflict(count: &StackCount) -> bool {
    let differ = count.exits.windows(2).any(|pair| pair[0] != pair[1]);
    differ || count.exits.iter().any(|&exit| exit > 1)
}

fn count_with_guess(
    graph: &OpGraph,
    call_targets: &CallTargets,
    effects: &[X87Effect],
    virtual_call_guess: Depth,
) -> FunctionAnalysis {
    let counter = StackCounter {
        graph,
        call_targets,
        effects,
        virtual_call_guess,
    };

    let before_calls = find_ops_before_any_call(graph, call_targets, effects);
    let X87Arguments { args, count } = find_x87_arguments(&counter, &before_calls);
    let mut opaque = count.opaque;

    // A read we still can't explain: some call returned more than we think.
    if !count.missing_values.is_empty() {
        opaque = opaque.or(Some(OpaqueReason::MissingValuesAfterCall));
    }

    // A called function takes values we never loaded.
    // Hidden x87 arguments if before any call, else who knows.
    let mut hidden_args = false;
    for &op in &count.calls_missing_values {
        if before_calls[op] {
            hidden_args = true;
        } else {
            opaque = opaque.or(Some(OpaqueReason::MissingValuesAfterCall));
        }
    }

    if exits_conflict(&count) {
        opaque = opaque.or(Some(OpaqueReason::ExitConflict));
    }

    FunctionAnalysis {
        count,
        args,
        hidden_args,
        opaque,
        entry: None,
    }
}
