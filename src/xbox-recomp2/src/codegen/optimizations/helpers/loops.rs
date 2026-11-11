//! Finds the loops in one function.
//!
//! See `helpers/loops.md` for more info.

use super::op_graph::{Edge, OpGraph, OpIndex, Predecessors};

/// An op's position in the [`OpGraph`].
type OpId = u32;

/// A position in [`LoopFinder::jumps_back`].
type JumpIndex = u32;

/// Finds the loops in a function.
#[derive(Default)]
pub(in crate::codegen::optimizations) struct LoopFinder {
    /// What we know about each op.
    ///
    /// - Index: [`OpIndex`].
    /// - Value: how far the search got with the op, and which loop it's in.
    ops: Vec<OpState>,

    /// Stack of ops we're searching from.
    ///
    /// - Index: depth of the search, with the entry at 0.
    /// - Value: the op, and how many of its next ops we tried.
    search: Vec<SearchStep>,

    /// Each jump to an op we passed on the way there.
    ///
    /// - Index: [`JumpIndex`], in the order we found them.
    /// - Value: where the jump starts and lands.
    jumps_back: Vec<JumpBack>,

    /// Stack of ops whose predecessors still need collecting.
    ///
    /// - Index: order we found them; we take from the end.
    /// - Value: the op.
    pending: Vec<OpId>,

    /// The ops of the loop we're collecting.
    ///
    /// - Index: order we collected them, with the header at 0.
    /// - Value: the op.
    body: Vec<OpId>,
}

/// What we know about one op.
#[derive(Clone, Copy)]
struct OpState {
    /// How far the search from the entry got with it.
    visit: Visit,

    /// The jump back whose loop it was last collected into.
    collected_in: JumpIndex,
}

/// An op we're searching from.
#[derive(Clone, Copy)]
struct SearchStep {
    op: OpId,

    /// How many of the ops that can run right after it we tried.
    tried: u32,
}

/// A jump to an op we passed on the way there.
#[derive(Clone, Copy)]
struct JumpBack {
    from: OpId,

    /// The op the loop starts at, called its header.
    to: OpId,
}

/// How far the search from the entry got with an op.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    /// Not reached.
    Unreached,

    /// Passed on the way to the op we're at.
    Searching,

    /// Followed everything that runs after it.
    Finished,
}

impl LoopFinder {
    /// Calls `visit` with the ops of each loop in `graph`.
    ///
    /// # Arguments
    ///
    /// - `graph`: the function.
    /// - `visit`: gets the op indices of one loop, header first.
    ///
    /// # Returns
    ///
    /// `false` if:
    ///
    /// - We can't follow the function's jumps.
    /// - Something jumps into the middle of a loop.
    pub(in crate::codegen::optimizations) fn for_each_loop(
        &mut self,
        graph: &OpGraph,
        mut visit: impl FnMut(&[OpId]),
    ) -> bool {
        // Bail out if there's code we can't follow.
        if (0..graph.len()).any(|op| graph.edges(op).contains(&Edge::Unknown)) {
            return false;
        }

        // Now scan the instructions, find jumps back to start.
        self.find_jumps_back(graph);
        if self.jumps_back.is_empty() {
            return true;
        }

        let count = graph.len();
        let links = (0..count).flat_map(|op| {
            let successors = graph.local_successors(op);
            successors.map(move |next| (op as OpId, next))
        });
        let predecessors = Predecessors::new(count, links);

        // Collect the loop each jump back closes.
        for jump in 0..self.jumps_back.len() {
            if !self.collect_body(graph, &predecessors, jump as JumpIndex) {
                return false;
            }

            visit(&self.body);
        }

        true
    }

    /// Finds each jump to an op we passed on the way there.
    ///
    /// # Remarks
    ///
    /// This is a depth-first search from the entry.
    /// Every loop the entry can reach has at least one such jump.
    fn find_jumps_back(&mut self, graph: &OpGraph) {
        // Reset buffers.
        self.jumps_back.clear();
        self.ops.clear();
        self.ops.resize(
            graph.len(),
            OpState {
                visit: Visit::Unreached,
                collected_in: JumpIndex::MAX,
            },
        );

        // Start with initial op (entry)
        self.ops[graph.entry as OpIndex].visit = Visit::Searching;
        self.search.push(SearchStep {
            op: graph.entry,
            tried: 0,
        });

        // Pop last from stack (the 'depth first')
        while let Some(&SearchStep { op, tried }) = self.search.last() {
            // Move on once we've tried everything that runs after `op`.
            let next = match graph.local_successors(op as OpIndex).nth(tried as usize) {
                Some(next) => next,
                None => {
                    // Instruction doesn't go anywhere, so this 'node' is 'finished'.
                    self.ops[op as OpIndex].visit = Visit::Finished;
                    self.search.pop();
                    continue;
                }
            };

            self.search.last_mut().unwrap().tried += 1;

            match self.ops[next as OpIndex].visit {
                // Visiting this node. Record the jump back.
                Visit::Searching => self.jumps_back.push(JumpBack { from: op, to: next }),
                Visit::Finished => {}
                // New node. Add it to search.
                Visit::Unreached => {
                    self.ops[next as OpIndex].visit = Visit::Searching;
                    self.search.push(SearchStep { op: next, tried: 0 });
                }
            }
        }
    }

    /// Collects the loop closed by `jumps_back[jump]` into `body`.
    ///
    /// # Returns
    ///
    /// `false` if something jumps into the middle of the loop.
    fn collect_body(
        &mut self,
        graph: &OpGraph,
        predecessors: &Predecessors,
        jump: JumpIndex,
    ) -> bool {
        let JumpBack { from, to: header } = self.jumps_back[jump as usize];
        self.body.clear();
        self.body.push(header);
        self.ops[header as OpIndex].collected_in = jump;

        // Walk back from the jump, and stop at the header.
        self.pending.clear();
        self.pending.push(from);
        while let Some(op) = self.pending.pop() {
            if self.ops[op as OpIndex].collected_in == jump {
                continue;
            }

            // We walked back to the entry, somehow...
            // So something jumps into the middle of the loop.
            if op == graph.entry {
                return false;
            }

            self.ops[op as OpIndex].collected_in = jump;
            self.body.push(op);
            for previous in predecessors.of(op as OpIndex) {
                // Code the entry can't reach never runs.
                if self.ops[previous].visit != Visit::Unreached {
                    self.pending.push(previous as OpId);
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    /// The addresses of each loop in `code`, or [`None`] if we can't follow it.
    fn loops(code: &[u8]) -> Option<Vec<Vec<u32>>> {
        // Decode the code.
        let model = model_of(code);
        let graph = OpGraph::new(&model)?;

        // Find the loops.
        let mut loops = Vec::new();
        let followed = LoopFinder::default().for_each_loop(&graph, |body| {
            // Convert op back into addresses.
            let ops = body.iter().map(|&op| graph.ops[op as OpIndex]);
            let addresses = ops.flat_map(|op| op.guests().map(|guest| guest.address.get()));
            loops.push(addresses.collect());
        });

        // If whole function followed, return loops.
        followed.then_some(loops)
    }

    #[rstest]
    #[case::one_loop(
        &[
            0x90,       // 0x1000: nop
            0x48,       // 0x1001: dec  eax    ; header
            0x75, 0xFD, // 0x1002: jnz  0x1001 ; jumps back
            0xC3,       // 0x1004: ret
        ],
        Some(vec![vec![0x1001, 0x1002]]),
    )]
    #[case::second_way_in(
        &[
            0xE3, 0x01, // 0x1000: jecxz 0x1003 ; skips the header <-
            0x40,       // 0x1002: inc  eax     ; header
            0x4A,       // 0x1003: dec  edx
            0x75, 0xFC, // 0x1004: jnz  0x1002
            0xC3,       // 0x1006: ret
        ],
        None,
    )]
    fn for_each_loop_should_collect_bodies(
        #[case] code: &[u8],
        #[case] expected: Option<Vec<Vec<u32>>>,
    ) {
        // Act
        let actual = loops(code);

        // Assert
        assert_eq!(actual, expected);
    }
}
