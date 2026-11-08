//! Finds the eflags needed before and after each operation in one function.
//!
//! See "How it works" in `needed-flags.md`.

use super::predecessors::Predecessors;
use crate::codegen::function::model::{ALL_ARITHMETIC_FLAGS, ControlFlow, Edge, FunctionModel, Op};
use crate::codegen::optimizations::helpers::flag_summary::FlagSummary;
use crate::codegen::optimizations::helpers::instruction::flag_effects::EFlagEffects;
use crate::codegen::optimizations::helpers::known_callers::direct_call_target;
use crate::codegen::optimizations::helpers::workqueue::WorkQueue;
use crate::decode::CpuFlag;
use xbox_recomp2_executable::GuestAddress32;

/// Works out which eflags each operation in one function needs.
///
/// # Remarks
///
/// Build it once, then call [`FlagGraph::walk`] for each set of assumptions
/// about callers and callees.
pub(in crate::codegen) struct FlagGraph<'a> {
    model: &'a FunctionModel,
    flow: ControlFlow,
    predecessors: Predecessors,

    /// What each operation reads and writes.
    /// Calls and returns are placeholders; [`FlagGraph::step`] fills them in.
    fixed: Vec<EFlagEffects>,

    /// See [`hooked_ops`].
    hooked: Vec<bool>,
}

/// The results of [`FlagGraph::walk`].
///
/// # Remarks
///
/// Reuse one between walks to keep its allocations.
#[derive(Default)]
pub(in crate::codegen) struct Solution {
    /// Flags needed before each operation.
    pub live_before: Vec<CpuFlag>,

    /// Flags needed after each operation.
    pub live_after: Vec<CpuFlag>,
}

/// What one operation does with flags in the current walk.
#[derive(Clone, Copy)]
struct Step {
    /// Flags this operation reads and writes.
    effects: EFlagEffects,

    /// Flags needed by paths that leave the function here.
    exit_flags: CpuFlag,
}

impl<'a> FlagGraph<'a> {
    /// Builds the graph for `model`.
    ///
    /// # Returns
    ///
    /// [`None`] when the function has no entry instruction.
    pub(in crate::codegen) fn new(model: &'a FunctionModel) -> Option<Self> {
        // Find where each operation can go next.
        // Each path out of a branch can need different flags.
        let flow = ControlFlow::new(model)?;
        let predecessors = Predecessors::new(flow.len(), local_edges(&flow));
        let hooked = hooked_ops(model);
        let fixed = model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .map(|op| match op {
                Op::Guest(guest) => EFlagEffects::of(&guest.insn),
                Op::CdqIdiv(idiv) => EFlagEffects::of(&idiv.guest.insn),
                Op::MergedCmpJmp(pair) => EFlagEffects::of(&pair.compare.insn),
                Op::Failed { .. } => EFlagEffects::barrier(),
                Op::Call(_) | Op::Return(_) => EFlagEffects::none(),
            })
            .collect();

        Some(Self {
            model,
            flow,
            predecessors,
            fixed,
            hooked,
        })
    }

    /// The function's flow.
    pub(in crate::codegen) fn flow(&self) -> &ControlFlow {
        &self.flow
    }

    /// The function's flow, without the rest of the graph.
    pub(super) fn into_flow(self) -> ControlFlow {
        self.flow
    }

    /// Flags needed when the function starts, according to `solution`.
    pub(in crate::codegen) fn entry(&self, solution: &Solution) -> CpuFlag {
        solution.live_before[self.flow.entry as usize]
    }

    /// Works out the flags needed before and after each operation.
    ///
    /// # Arguments
    ///
    /// - `at_return`: flags needed after each `ret`.
    /// - `flag_summary`: what calling or jumping to a function does with flags.
    ///   Gets [`None`] for an indirect call or jump.
    /// - `solution`: where the results go, replacing its old contents.
    pub(in crate::codegen) fn walk(
        &self,
        at_return: CpuFlag,
        flag_summary: impl Fn(Option<GuestAddress32>) -> FlagSummary,
        solution: &mut Solution,
    ) {
        let ops = self.model.blocks.iter().flat_map(|block| &block.ops);
        let steps: Vec<_> = ops
            .enumerate()
            .map(|(index, op)| self.step(index, op, at_return, &flag_summary))
            .collect();

        let count = self.flow.len();
        solution.live_before.clear();
        solution.live_before.resize(count, CpuFlag::empty());
        solution.live_after.clear();
        solution.live_after.resize(count, CpuFlag::empty());

        // Start at the end of the function and work up.
        //
        // je  skip        ; before: ZF, CF, SF  <- writes(none), reads(ZF)
        //                 ; after:  CF, SF      <- adc needs CF, sets needs SF
        // adc ecx, 0
        // skip:
        // sets dl
        //
        // Read the markdown needed-flags.md for a better understanding.
        let mut pending = WorkQueue::<usize>::full(count);
        while let Some(index) = pending.pop() {
            let step = steps[index];

            // 1. Needed after: what anything next needs, plus what exits need.
            let mut after = step.exit_flags;
            for edge in self.flow.edges(index) {
                if let Edge::Op(next) = *edge {
                    after |= solution.live_before[next as usize];
                }
            }
            solution.live_after[index] = after;

            // 2. Needed before: needed after, minus writes, plus reads.
            let before = (after & !step.effects.writes) | step.effects.reads;

            // 3. Recheck: if `before` changed, recheck whatever can run before this.
            if before == solution.live_before[index] {
                continue;
            }

            solution.live_before[index] = before;
            for previous in self.predecessors.of(index) {
                pending.push(previous);
            }
        }
    }

    /// What the operation at `index` does with flags in this walk.
    fn step(
        &self,
        index: usize,
        op: &Op,
        at_return: CpuFlag,
        flag_summary: &impl Fn(Option<GuestAddress32>) -> FlagSummary,
    ) -> Step {
        let mut exit_flags = CpuFlag::empty();
        for &edge in self.flow.edges(index) {
            exit_flags |= leaving(edge, at_return, flag_summary);
        }

        let mut effects = match op {
            // Reads what the called function reads.
            // Writes every flag it doesn't keep.
            //
            // add  eax, ebx  ; after: CF  <- sub_A reads CF
            // call sub_A     ; reads CF, keeps none
            Op::Call(call) => {
                let target = direct_call_target(&call.guest).map(|target| target.0.into());
                let summary = flag_summary(target);
                EFlagEffects {
                    reads: summary.reads,
                    writes: ALL_ARITHMETIC_FLAGS & !summary.keeps,
                }
            }

            // `ret` leaves the function, so it needs what our caller needs.
            Op::Return(_) => {
                exit_flags |= at_return;
                EFlagEffects::none()
            }
            _ => self.fixed[index],
        };

        // A hook runs first and gets `ctx`, so it can read every flag.
        // Pray to Aidios Clang can resolve it.
        if self.hooked[index] {
            effects.reads |= ALL_ARITHMETIC_FLAGS;
        }

        Step {
            effects,
            exit_flags,
        }
    }
}

/// Flags needed on `edge` if it leaves the function.
pub(super) fn leaving(
    edge: Edge,
    at_return: CpuFlag,
    flag_summary: &impl Fn(Option<GuestAddress32>) -> FlagSummary,
) -> CpuFlag {
    match edge {
        Edge::Op(_) | Edge::Abort => CpuFlag::empty(),
        Edge::Function(target) => flag_summary(Some(target)).before(at_return),
        Edge::Indirect => flag_summary(None).before(at_return),
        Edge::Unknown => ALL_ARITHMETIC_FLAGS,
    }
}

/// Whether a hook runs before each operation, across all blocks in order.
fn hooked_ops(model: &FunctionModel) -> Vec<bool> {
    model
        .blocks
        .iter()
        .flat_map(|block| &block.ops)
        .map(|op| model.hooks.iter().any(|hook| op.has_address(hook.address)))
        .collect()
}

/// Each `(from, to)` pair where `to` can run right after `from`.
fn local_edges(flow: &ControlFlow) -> impl Iterator<Item = (u32, u32)> + '_ {
    (0..flow.len()).flat_map(move |index| {
        local_targets(flow.edges(index)).map(move |target| (index as u32, target))
    })
}

/// Where `edges` go inside the function.
fn local_targets(edges: &[Edge]) -> impl Iterator<Item = u32> + '_ {
    edges.iter().filter_map(|edge| match *edge {
        Edge::Op(target) => Some(target),
        _ => None,
    })
}
