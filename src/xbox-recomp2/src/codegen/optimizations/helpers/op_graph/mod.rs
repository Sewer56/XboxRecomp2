//! One function's ops, numbered across all its blocks, and where each can go next.
//!
//! - [`OpGraph::new`] builds the graph for a [`FunctionModel`].
//! - [`OpGraph::edges`] returns every way one op can continue, including jumps
//!   out of the function.
//! - [`OpGraph::hook`] returns the mid-assembly hook that runs before an op.

use crate::codegen::function::model::{
    Entry, FunctionModel, GuestOp, MergedCmpJmp, MergedInstructionKind, Op,
};
use crate::utils::flat_lists::FlatLists;
use op_resolver::{BlockIndex, LabelAddress, OpResolver};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

mod edges;
mod op_resolver;

/// An op's position in an [`OpGraph`], across all blocks of a function.
pub(in crate::codegen) type OpIndex = usize;

/// One function's ops, and where each can go next.
pub(in crate::codegen::optimizations) struct OpGraph<'a> {
    /// The ops, across all blocks in order.
    pub ops: Vec<&'a Op>,

    /// Whether each op starts a labelled block, where jumps land.
    pub labelled: Vec<bool>,

    /// The first op to run.
    ///
    /// In the rare cases where first/lowest address is not the function's
    /// entry (i.e. a jump backwards)
    pub entry: u32,

    /// Each op's edges, the paths the op can continue.
    edges: FlatLists<Edge>,

    /// The ops each op's edges go to.
    op_targets: FlatLists<u32>,

    /// The mid-assembly hook before each hooked op.
    hooks: BTreeMap<u32, OpHook>,
}

/// One way an op can continue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) enum Edge {
    /// Runs the op at this index next.
    Op(u32),

    /// Jumps to code outside this function, like a tail call to `0x2000`.
    Function(GuestAddress32),

    /// Aborts, so no guest code runs after it.
    Abort,

    /// An indirect jump, like `jmp eax`, to code we can't name.
    Indirect,

    /// We don't know what runs next, such as after a decode failure.
    Unknown,
}

/// A guest instruction in an op, from [`OpGraph::for_each_instruction`].
#[derive(Clone, Copy)]
pub(in crate::codegen::optimizations) struct Instruction<'a> {
    pub guest: &'a GuestOp,

    /// Where it jumps, if it's a jump merged into a [`MergedCmpJmp`].
    ///
    /// [`MergedCmpJmp`]: crate::codegen::function::model::MergedCmpJmp
    pub jump: Option<Edge>, // Note: This is a but fudged. Spaghetti. Needs to be represented better - Sewer!
}

impl<'a> Instruction<'a> {
    /// An instruction that doesn't jump.
    fn plain(guest: &'a GuestOp) -> Self {
        Self { guest, jump: None }
    }
}

/// Where control can go after a mid-assembly hook, besides its instruction.
pub(in crate::codegen::optimizations) struct OpHook {
    /// Whether the hook may return from the function.
    pub may_return: bool,

    /// Ops the hook may jump to.
    pub jumps: Vec<u32>,
}

impl<'a> OpGraph<'a> {
    /// Builds the graph for `model`.
    ///
    /// # Returns
    ///
    /// [`None`] if the function has no entry, or a hook jumps to a label with
    /// no op after it.
    pub(in crate::codegen::optimizations) fn new(model: &'a FunctionModel) -> Option<Self> {
        let resolver = OpResolver::new(model);

        // Go to first instruction in method.
        let entry = resolver.first_op(match model.entry {
            Entry::First => BlockIndex(0),
            Entry::Goto(label) => resolver.block_at(LabelAddress(label))?,
            Entry::Missing(_) => return None,
        })?;

        // Number the ops, across all blocks in order.
        let count = resolver.instruction_count as usize;
        let mut ops = Vec::with_capacity(count);
        let mut labelled = Vec::with_capacity(count);
        for block in &model.blocks {
            for (index, op) in block.ops.iter().enumerate() {
                labelled.push(index == 0 && block.label.is_some());
                ops.push(op);
            }
        }

        // Find where each op can go, and which ops have a hook.
        let (edges, op_targets) = edges::find(model, &resolver);
        let hooks = find_hooks(model, &ops, &resolver)?;

        Some(Self {
            ops,
            labelled,
            entry,
            edges,
            op_targets,
            hooks,
        })
    }

    /// Number of ops.
    pub(in crate::codegen::optimizations) fn len(&self) -> usize {
        self.ops.len()
    }

    /// Number of edges across all ops.
    pub(in crate::codegen::optimizations) fn edges_len(&self) -> usize {
        self.edges.item_count()
    }

    /// Returns every way op `index` can continue.
    ///
    /// # Returns
    ///
    /// - Empty for a return.
    /// - For a conditional jump, the jump's edge, then the next op's.
    /// - For a merged cmp/jmp, each merged jump's edge in order, then the next
    ///   op's.
    /// - For a switch, each distinct local case, then any case leaving the function.
    ///
    /// # Remarks
    ///
    /// These are the ways out after the op's own instructions.
    /// Its hook runs before them; see [`Self::hook`].
    pub(in crate::codegen::optimizations) fn edges(&self, index: OpIndex) -> &[Edge] {
        self.edges.get(index)
    }

    /// The ops that op `index`'s edges go to.
    ///
    /// Edges that leave the function are left out.
    pub(in crate::codegen::optimizations) fn op_targets(&self, index: OpIndex) -> &[u32] {
        self.op_targets.get(index)
    }

    /// Returns the ops that can run after op `index`, if it stays in the function.
    ///
    /// # Returns
    ///
    /// - Empty for a return.
    /// - One op if it always goes to the same place.
    /// - For a conditional jump, the jump target, then the next op.
    /// - For a switch, each distinct case, in index order.
    /// - [`None`] if unknown, or it leaves the function without returning.
    pub(in crate::codegen::optimizations) fn successors(&self, index: OpIndex) -> Option<&[u32]> {
        let local = self
            .edges(index)
            .iter()
            .all(|edge| matches!(edge, Edge::Op(_)));
        local.then(|| self.op_targets(index))
    }

    /// The mid-assembly hook that runs before op `index`, if it has one.
    pub(in crate::codegen::optimizations) fn hook(&self, index: OpIndex) -> Option<&OpHook> {
        self.hooks.get(&(index as u32))
    }

    /// Ops that can run right after op `index`, including where its hook jumps.
    pub(in crate::codegen::optimizations) fn local_successors(
        &self,
        index: OpIndex,
    ) -> impl Iterator<Item = u32> + '_ {
        let hook_jumps = self.hook(index).map_or(&[][..], |hook| &hook.jumps[..]);
        self.op_targets(index).iter().chain(hook_jumps).copied()
    }

    /// Computes a value for each op. (loops backwards)
    /// Repeats until no value changes since last run.
    ///
    /// # Arguments
    ///
    /// - `initial`: the value every op starts with on first loop.
    /// - `update`: func to get op's new value. Based on values so far.
    ///
    /// # Returns
    ///
    /// Each op's final value.
    ///
    /// # Remarks
    ///
    /// This is basically the 'loop/repeat until settled/fixed' that's mentioned
    /// in a few of the optimisation docs on the wiki.
    pub(in crate::codegen::optimizations) fn sweep_backward<T: Copy + Eq>(
        &self,
        initial: T,
        mut update: impl FnMut(OpIndex, &[T]) -> T,
    ) -> Vec<T> {
        let mut values = vec![initial; self.len()];

        // Repeat until nothing changes.
        let mut changed = true;
        while changed {
            changed = false;
            for op in (0..self.len()).rev() {
                let value = update(op, &values);
                if value != values[op] {
                    values[op] = value;
                    changed = true;
                }
            }
        }

        values
    }

    /// Calls `visit` with each guest instruction in op `index`, in order.
    ///
    /// [`MergedCmpJmp`]: crate::codegen::function::model::MergedCmpJmp
    pub(in crate::codegen::optimizations) fn for_each_instruction(
        &self,
        index: OpIndex,
        mut visit: impl FnMut(Instruction<'a>),
    ) {
        match self.ops[index] {
            Op::MergedCmpJmp(merged) => self.for_each_merged_cmp_jmp(index, merged, visit),
            op => {
                for guest in op.guests() {
                    visit(Instruction::plain(guest));
                }
            }
        }
    }

    /// [`Self::for_each_instruction`] for a merged cmp/jmp, whose jumps are mid-op.
    fn for_each_merged_cmp_jmp(
        &self,
        index: OpIndex,
        merged: &'a MergedCmpJmp,
        mut visit: impl FnMut(Instruction<'a>),
    ) {
        // The compare and x87 pop never jump.
        visit(Instruction::plain(&merged.compare));
        if let Some(pop) = &merged.x87_pop {
            visit(Instruction::plain(pop));
        }

        // Now handle the rest of the instructions.
        let mut jumps = self.edges(index).iter().copied();
        for instruction in &merged.instructions {
            let guest = &instruction.guest;
            let jump = match instruction.kind {
                MergedInstructionKind::Jump(_) => jumps.next(),
                _ => None,
            };

            visit(Instruction { guest, jump });
        }
    }

    /// The edges op `index` takes after its last instruction.
    ///
    /// For a [`MergedCmpJmp`], that's only the fall-through.
    ///
    /// [`MergedCmpJmp`]: crate::codegen::function::model::MergedCmpJmp
    pub(in crate::codegen::optimizations) fn end_edges(&self, index: OpIndex) -> &[Edge] {
        let edges = self.edges(index);
        match self.ops[index] {
            // The fall-through is always its last edge.
            Op::MergedCmpJmp(_) => &edges[edges.len() - 1..],
            _ => edges,
        }
    }
}

/// Finds the op each of `model`'s hooks runs before, and the ops it can jump to.
///
/// # Returns
///
/// The hooks, by op. [`None`] if one jumps to a label with no op after it.
fn find_hooks(
    model: &FunctionModel,
    ops: &[&Op],
    resolver: &OpResolver,
) -> Option<BTreeMap<u32, OpHook>> {
    let mut hooks = BTreeMap::new();
    for hook in &model.hooks {
        let at = match ops.iter().position(|op| op.has_address(hook.address)) {
            Some(at) => at,
            None => continue,
        };

        let mut jumps = Vec::with_capacity(hook.jumps.len());
        for &target in &hook.jumps {
            jumps.push(resolver.op_at(LabelAddress(target))?);
        }

        let hook = OpHook {
            may_return: hook.may_return,
            jumps,
        };

        hooks.insert(at as u32, hook);
    }

    Some(hooks)
}
