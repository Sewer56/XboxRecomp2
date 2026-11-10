//! Works out which way string instructions like `rep movsd` run.
//!
//! See `find-string-direction.md` for more info.

use super::helpers::op_graph::{Edge, OpGraph, OpIndex};
use super::helpers::workqueue::WorkQueue;
use crate::codegen::function::model::{DirectionFlag, FunctionModel, Op};
use crate::decode::{CpuFlag, Insn, Mnemonic};
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

/// DF before each operation of one function, in block order.
/// [`None`] for operations nothing reaches.
/// (👆 this kinda sucks lol, but so does special value on the enum itself)
type DfBeforeOps = Vec<Option<DirectionFlag>>;

pub(super) fn run(models: &mut [FunctionModel], diag: &mut dyn Write) {
    // Find DF before each operation, in every function.
    let mut df_per_function: Vec<DfBeforeOps> = Vec::with_capacity(models.len());
    for model in models.iter() {
        match analyze_function(model) {
            Ok(before) => df_per_function.push(before),
            Err(address) => {
                // Compiler emitted DF=1 going into function.
                // So we can't trust it to always have DF=0 on entry.
                // Bail out.
                let _ = writeln!(
                    diag,
                    "WARNING: DF may be set after 0x{address:X}; \
                     string instructions will read DF at runtime"
                );
                return;
            }
        }
    }

    // Save it on each instruction.
    for (model, before) in models.iter_mut().zip(df_per_function) {
        apply_to_each_instruction(model, &before);
    }
}

/// Finds DF before each operation of `model`.
///
/// # Returns
///
/// Empty if the function has no entry.
///
/// # Errors
///
/// The address of an operation that may leave the function with DF set,
/// like a `ret` after `std`.
fn analyze_function(model: &FunctionModel) -> Result<DfBeforeOps, GuestAddress32> {
    let graph = match OpGraph::new(model) {
        Some(graph) => graph,
        None => return Ok(Vec::new()),
    };

    // Functions start with DF clear.
    let mut before = vec![None; graph.len()];
    before[graph.entry as OpIndex] = Some(DirectionFlag::Forward);

    // Follow DF along every path.
    let mut queue = WorkQueue::<OpIndex>::full(graph.len());
    let mut df_on_edges = Vec::with_capacity(2); // <- most operations have 1 or 2 edges
    while let Some(index) = queue.pop() {
        let df = match before[index] {
            Some(df) => df,
            None => continue,
        };

        // If DF=1 before a call, or return; we assume it's a foreign compiler,
        // not MSVC on Xbox which wouldn't do that. So bail out.
        let op = graph.ops[index];
        if matches!(op, Op::Call(_) | Op::Return(_)) && df != DirectionFlag::Forward {
            return Err(op.address());
        }

        // Compute df on each edge of an instruction.
        df_on_each_edge(&graph, index, df, &mut df_on_edges);

        // Now get instruction indices, etc. for each edge.
        // And update DF accordingly.
        for (edge, &after) in graph.edges(index).iter().zip(&df_on_edges) {
            match *edge {
                Edge::Op(next) => {
                    let next = next as OpIndex;
                    let joined = df_where_paths_meet(before[next], after); // if they disagree, we get *Unknown*

                    if before[next] != Some(joined) {
                        before[next] = Some(joined);
                        queue.push(next); // analyze each branch/edge
                    }
                }
                Edge::Abort => {}

                // Jumps out of the function, like `jmp eax`.
                // Bail out.
                Edge::Function(_) | Edge::Indirect | Edge::Unknown => {
                    if after != DirectionFlag::Forward {
                        return Err(op.address());
                    }
                }
            }
        }
    }

    Ok(before)
}

/// Fills `df_on_edges` with DF on each of op `index`'s edges, given DF before it.
///
/// Edges are in [`OpGraph::edges`] order.
fn df_on_each_edge(
    graph: &OpGraph,
    index: OpIndex,
    before: DirectionFlag,
    df_on_edges: &mut Vec<DirectionFlag>,
) {
    df_on_edges.clear();

    // Step DF through the op.
    let mut df = before;
    graph.for_each_instruction(index, |instruction| {
        if instruction.jump.is_some() {
            df_on_edges.push(df);
        }

        df = df_after(df, &instruction.guest.insn);
    });

    // Functions return with DF clear.
    if let Op::Call(_) = graph.ops[index] {
        df = DirectionFlag::Forward;
    }

    // The other edges leave after the last instruction.
    df_on_edges.resize(graph.edges(index).len(), df);
}

/// Saves DF before each guest instruction of `model`.
fn apply_to_each_instruction(model: &mut FunctionModel, before: &DfBeforeOps) {
    let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);

    for (op, df) in ops.zip(before) {
        let mut df = df.unwrap_or(DirectionFlag::Unknown);

        match op {
            Op::Guest(guest) => guest.direction = df,
            Op::MergedCmpJmp(merged) => {
                for instruction in &mut merged.instructions {
                    instruction.guest.direction = df;
                    df = df_after(df, &instruction.guest.insn);
                }
            }
            Op::Call(_) | Op::Return(_) | Op::CdqIdiv(_) | Op::Failed { .. } => {}
        }
    }
}

/// DF after `insn`, given DF before it.
fn df_after(before: DirectionFlag, insn: &Insn) -> DirectionFlag {
    let flags = insn.instruction().cpu_flags;
    match insn.mnemonic() {
        Mnemonic::STD => DirectionFlag::Backward,
        Mnemonic::CLD => DirectionFlag::Forward,
        // Loads saved flags, like `popfd` or `iretd`.
        _ if (flags.modified | flags.undefined).contains(CpuFlag::DF) => DirectionFlag::Unknown,
        _ => before,
    }
}

/// DF where two paths meet. Known only if both agree.
fn df_where_paths_meet(old: Option<DirectionFlag>, new: DirectionFlag) -> DirectionFlag {
    match old {
        None => new,
        Some(old) if old == new => old,
        Some(_) => DirectionFlag::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use crate::decode::InstructionCategory;
    use core::slice;
    use rstest::rstest;

    const FORWARD: DirectionFlag = DirectionFlag::Forward;
    const BACKWARD: DirectionFlag = DirectionFlag::Backward;
    const UNKNOWN: DirectionFlag = DirectionFlag::Unknown;

    /// DF before each string instruction, in order.
    fn df_at_string_instructions(model: &FunctionModel) -> Vec<DirectionFlag> {
        let mut directions = Vec::new();
        for op in model.blocks.iter().flat_map(|block| &block.ops) {
            if let Op::Guest(guest) = op
                && guest.insn.instruction().meta.category == InstructionCategory::STRINGOP
            {
                directions.push(guest.direction);
            }
        }
        directions
    }

    // ── Known direction ──────────────────────────────────────────────────────

    #[rstest]
    #[case::forward(
        &[
            0xF3, 0xA5,   //     rep movsd       ; DF clear at entry
            0xC3,         //     ret
        ],
        &[FORWARD],
    )]
    #[case::std_then_cld(
        &[
            0xFD,         //     std
            0xF3, 0xA5,   //     rep movsd       ; backward
            0xFC,         //     cld
            0xF3, 0xAB,   //     rep stosd       ; forward again
            0xC3,         //     ret
        ],
        &[BACKWARD, FORWARD],
    )]
    #[case::paths_disagree(
        &[
            0x74, 0x01,   //     jz   .copy      ; DF clear
            0xFD,         //     std             ; DF set
                          // .copy:
            0xF3, 0xA5,   //     rep movsd       ; either
            0xFC,         //     cld
            0xC3,         //     ret
        ],
        &[UNKNOWN],
    )]
    #[case::popfd(
        &[
            0x9D,         //     popfd           ; DF from the stack
            0xF3, 0xA5,   //     rep movsd
            0xFC,         //     cld
            0xC3,         //     ret
        ],
        &[UNKNOWN],
    )]
    #[case::after_call(
        &[
            0xE8, 0x00, 0x00, 0x00, 0x00, // call .next    ; callee clears DF
                                          // .next:
            0xF3, 0xA5,                   //     rep movsd
            0xC3,                         //     ret
        ],
        &[FORWARD],
    )]
    fn run_should_find_direction_before_string_instructions(
        #[case] code: &[u8],
        #[case] expected: &[DirectionFlag],
    ) {
        // Arrange
        let mut model = model_of(code);

        // Act
        run(slice::from_mut(&mut model), &mut Vec::new());

        // Assert
        assert_eq!(df_at_string_instructions(&model), expected);
    }
}
