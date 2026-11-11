//! Finds the reads that a loop might wait on.

use crate::codegen::function::model::FunctionModel;
use crate::codegen::optimizations::helpers::loops::LoopFinder;
use crate::codegen::optimizations::helpers::op_graph::{OpGraph, OpIndex};
use loop_summary::LoopSummary;

mod loop_summary;

/// Finds the reads that a loop might wait on.
#[derive(Default)]
pub(super) struct WaitingReadFinder {
    /// Whether each op has a read that a loop might wait on.
    ///
    /// Index = Op
    /// Value = true/false
    pub waiting_reads: Vec<bool>,

    loops: LoopFinder,
    summary: LoopSummary,
}

impl WaitingReadFinder {
    /// Creates the finder.
    ///
    /// # Arguments
    ///
    /// - `nonvolatile_sse`: whether SSE loads skip `volatile` already. (the speed hack)
    pub(super) fn new(nonvolatile_sse: bool) -> Self {
        let mut finder = Self::default();
        finder.summary.nonvolatile_sse = nonvolatile_sse;
        finder
    }

    /// Finds the reads that a loop in `model` might wait on.
    ///
    /// # Returns
    ///
    /// `false` if we can't determine it.
    /// Otherwise [`Self::waiting_reads`] holds the result.
    pub(super) fn find(&mut self, model: &FunctionModel) -> bool {
        self.waiting_reads.clear();
        let graph = match OpGraph::new(model) {
            Some(graph) => graph,
            None => return false,
        };

        let Self {
            waiting_reads,
            loops,
            summary,
        } = self;

        loops.for_each_loop(&graph, |body| {
            if waiting_reads.is_empty() {
                waiting_reads.resize(graph.len(), false);
            }

            summary.clear();
            for &op in body {
                for guest in graph.ops[op as OpIndex].guests() {
                    summary.add(op, &guest.insn, model.ebp_frame);
                }
            }

            summary.mark_reads_at_fixed_address(waiting_reads);
        })
    }
}
