//! Which eflags must still be valid after each operation in a function.
//!
//! - [`NeededFlags`]: the results `eliminate_dead_flags` and `merge_cmp_jmp` read.
//! - [`FlagGraph`]: walks one function.
//!
//! See `needed-flags.md` for more info.

use super::flag_boundaries::FlagBoundaries;
use super::op_graph::{Edge, OpIndex};
use crate::codegen::function::model::FunctionModel;
use crate::decode::CpuFlag;
use crate::utils::flat_lists::{FlatLists, FlatListsBuilder};
use graph::{FlagGraph, Solution, leaving};

pub(in crate::codegen) mod graph;

/// The flags needed around each operation of one function.
pub(in crate::codegen::optimizations) struct NeededFlags {
    /// Flags needed after each operation, across all blocks in (seqquential) order.
    pub live_after: Vec<CpuFlag>,

    /// Flags needed on each path out of each operation.
    /// Can read all jump targets at once with [`NeededFlags::on_edges`].
    live_on_edges: FlatLists<CpuFlag>,
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
        let flags = FlagGraph::new(model)?;
        let at_return = boundaries.at_return(model.address.0.into());
        let flag_summary = |target| boundaries.flag_summary(target);
        let mut solution = Solution::default();
        flags.walk(at_return, flag_summary, &mut solution);

        // Work out what each path out of each operation needs.
        //
        // je  skip     ; taken:     SF  <- sets needs SF
        //              ; not taken: CF  <- adc needs CF
        let graph = flags.graph();
        let mut live_on_edges = FlatListsBuilder::with_capacity(graph.len(), graph.edges_len());
        for index in 0..graph.len() {
            let edges = graph.edges(index).iter();
            live_on_edges.push(edges.map(|&edge| match edge {
                Edge::Op(next) => solution.live_before[next as usize],
                edge => leaving(edge, at_return, &flag_summary),
            }));
        }

        Some(Self {
            live_after: solution.live_after,
            live_on_edges: live_on_edges.build(),
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
    /// One entry per path, in [`OpGraph::edges`] order.
    ///
    /// e.g. If we have a conditional jump like `je skip`, then there
    /// will be two results
    /// - taken
    /// - nyot~ taken
    ///
    /// [`OpGraph::edges`]: super::op_graph::OpGraph::edges
    pub(in crate::codegen::optimizations) fn on_edges(&self, index: OpIndex) -> &[CpuFlag] {
        self.live_on_edges.get(index)
    }
}
