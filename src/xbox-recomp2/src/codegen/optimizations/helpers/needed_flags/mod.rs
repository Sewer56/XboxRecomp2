//! Which eflags must still be valid after each operation in a function.
//!
//! - [`NeededFlags`]: the results `eliminate_dead_flags` and `merge_cmp_jmp` read.
//! - [`FlagGraph`]: walks one function.
//!
//! See `needed-flags.md` for more info.

use super::flag_boundaries::FlagBoundaries;
use crate::codegen::function::model::{ControlFlow, Edge, FunctionModel};
use crate::decode::CpuFlag;
use graph::{FlagGraph, Solution, leaving};

pub(in crate::codegen) mod graph;
mod predecessors;

/// The flags needed around each operation of one function.
pub(in crate::codegen::optimizations) struct NeededFlags {
    /// Flags needed after each operation, across all blocks in (seqquential) order.
    pub live_after: Vec<CpuFlag>,

    /// Flags needed on each path out of each operation.
    /// Can read all jump targets at once with [`NeededFlags::on_edges`].
    pub live_on_edges: Vec<CpuFlag>,

    /// The flow these results describe.
    pub flow: ControlFlow,
}

impl NeededFlags {
    /// Walks `model` with the [`FlagBoundaries`].
    ///
    /// # Returns
    ///
    /// [`None`] when the function has no entry instruction.
    pub(in crate::codegen::optimizations) fn new(
        model: &FunctionModel,
        boundaries: &FlagBoundaries,
    ) -> Option<Self> {
        let graph = FlagGraph::new(model)?;
        let at_return = boundaries.at_return(model.address.0.into());
        let flag_summary = |target| boundaries.flag_summary(target);
        let mut solution = Solution::default();
        graph.walk(at_return, flag_summary, &mut solution);

        // Work out what each path out of each operation needs.
        //
        // je  skip     ; taken:     SF  <- sets needs SF
        //              ; not taken: CF  <- adc needs CF
        let flow = graph.into_flow();
        let mut live_on_edges = vec![CpuFlag::empty(); flow.edges_len()];
        for index in 0..flow.len() {
            let range = flow.edge_range(index);
            for (slot, edge) in live_on_edges[range].iter_mut().zip(flow.edges(index)) {
                *slot = match *edge {
                    Edge::Op(next) => solution.live_before[next as usize],
                    edge => leaving(edge, at_return, &flag_summary),
                };
            }
        }

        Some(Self {
            live_after: solution.live_after,
            live_on_edges,
            flow,
        })
    }

    /// Flags needed on each path out of the operation at `index`.
    ///
    /// # Arguments
    ///
    /// - `index`: the operation's position across all blocks.
    ///
    /// # Returns
    ///
    /// One entry per path, in [`ControlFlow::edges`] order.
    ///
    /// e.g. If we have a conditional jump like `je skip`, then there
    /// will be two results
    /// - taken
    /// - nyot~ taken
    pub(in crate::codegen::optimizations) fn on_edges(&self, index: usize) -> &[CpuFlag] {
        &self.live_on_edges[self.flow.edge_range(index)]
    }
}
