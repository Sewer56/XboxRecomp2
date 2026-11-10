//! Skips the x87 condition-code (C0-C3) writes nothing reads.
//!
//! See `skip-unused-x87-status.md` for more info.

use super::helpers::instruction::effects::x87_status::{ConditionCodes, StatusEffect};
use super::helpers::instruction::fpu::is_x87;
use super::helpers::op_graph::{Edge, OpGraph, OpIndex};
use crate::codegen::function::model::{FunctionModel, Op};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

pub(super) fn run(models: &mut [FunctionModel]) {
    for model in models {
        // Skip functions without x87 (most of them)
        let ops = || model.blocks.iter().flat_map(|block| &block.ops);
        if !ops().flat_map(Op::guests).any(|guest| is_x87(&guest.insn)) {
            continue;
        }

        let needed_x87_addr_to_condcode = match OpGraph::new(model) {
            Some(graph) => needed_after(&graph),
            None => continue,
        };

        // Tell the writer to skip the rest.
        let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);
        for guest in ops.flat_map(Op::guests_mut) {
            if let Some(&codes) = needed_x87_addr_to_condcode.get(&guest.address) {
                guest.needed_status = codes;
            }
        }
    }
}

/// The condition codes read after each x87 instruction that writes some.
///
/// # Remarks
///
/// Writers with a hook, or inside a [`MergedCmpJmp`], aren't listed, so they
/// keep all codes.
///
/// [`MergedCmpJmp`]: crate::codegen::function::model::MergedCmpJmp
fn needed_after(graph: &OpGraph) -> BTreeMap<GuestAddress32, ConditionCodes> {
    // Example
    //
    //   fld  [ecx]     ; clears C1
    //   fmul [edx]     ; clears C1
    //   fadd [esi]     ; clears C1
    //   call foo       ; may read it
    //
    // Hey you! YES, YOU!
    //
    // This is kind of the same 'logic' as in eflags-elimination.
    // A read of the 'skip unused flags' section would be good
    // in the wiki. I'm in a bit of a hurry, soz.
    //
    // Wait- deja-vu? Have you read this in mmx_locals.rs already!? Nani!?

    let effects: Vec<StatusEffect> = (0..graph.len()).map(|op| op_effect(graph, op)).collect();

    // Find which codes may be read after each op.
    let needed_after = graph.sweep_backward(ConditionCodes::empty(), |op, needed_after| {
        let mut needed = ConditionCodes::empty();

        for &edge in graph.edges(op) {
            needed |= match edge {
                Edge::Op(next) => {
                    let next = next as OpIndex;
                    effects[next].needed_before(needed_after[next])
                }
                _ => ConditionCodes::all(),
            };
        }

        needed
    });

    // Note it for each writer.
    let mut by_address = BTreeMap::new();
    for op in 0..graph.len() {
        let guest = match graph.ops[op] {
            Op::Guest(guest) => guest,
            _ => continue,
        };

        if !effects[op].writes.is_empty() {
            by_address.insert(guest.address, needed_after[op]);
        }
    }

    by_address
}

/// What op `op` does to the condition codes.
fn op_effect(graph: &OpGraph, op: OpIndex) -> StatusEffect {
    let reads_all = StatusEffect {
        reads: ConditionCodes::all(),
        writes: ConditionCodes::empty(),
    };

    // A hook runs before the op, and may read them.
    if graph.hook(op).is_some() {
        return reads_all;
    }

    match graph.ops[op] {
        Op::Guest(guest) => StatusEffect::of(&guest.insn),
        Op::CdqIdiv(_) => StatusEffect::none(),
        // The callee, or our caller, may read them.
        Op::Call(_) | Op::Return(_) | Op::Failed { .. } => reads_all,
        // Assume a merged compare reads them all, rather than follow each jump.
        Op::MergedCmpJmp(_) => reads_all,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    const C0: ConditionCodes = ConditionCodes::C0;
    const C2: ConditionCodes = ConditionCodes::C2;
    const C3: ConditionCodes = ConditionCodes::C3;
    const NONE: ConditionCodes = ConditionCodes::empty();
    const ALL: ConditionCodes = ConditionCodes::all();

    /// By byte offset, the codes each status writer in `model` must write.
    fn writers(model: &FunctionModel) -> Vec<(u32, ConditionCodes)> {
        let ops = model.blocks.iter().flat_map(|block| &block.ops);
        ops.flat_map(Op::guests)
            .filter(|guest| !StatusEffect::of(&guest.insn).writes.is_empty())
            .map(|guest| (guest.address.get() - model.address.0, guest.needed_status))
            .collect()
    }

    /// Runs [`run`] on `code`, then lists its status writers as in [`writers`].
    fn needed(code: &[u8]) -> Vec<(u32, ConditionCodes)> {
        let mut model = model_of(code);
        run(core::slice::from_mut(&mut model));
        writers(&model)
    }

    // ── Liveness ─────────────────────────────────────────────────────────────

    #[rstest]
    #[case::run_before_ret(
        &[
            0xD9, 0x01,             // 0: fld   [ecx]    ; <- dead: fmul clears C1 again
            0xD8, 0x0A,             // 2: fmul  [edx]    ; <- dead: fadd clears C1 again
            0xD8, 0x06,             // 4: fadd  [esi]    ; ret may hand them on
            0xC3,                   // 6: ret
        ],
        &[(0, C0 | C2 | C3), (2, C0 | C2 | C3), (4, ALL)],
    )]
    #[case::read_by_fnstsw(
        &[
            0xD9, 0x01,             // 0:  fld    [ecx]  ; <- dead: fabs clears C1 again
            0xD9, 0xE1,             // 2:  fabs          ; fnstsw reads it
            0xDF, 0xE0,             // 4:  fnstsw ax
            0xD9, 0xE8,             // 6:  fld1          ; clears C1 again before ret
            0xDD, 0xD8,             // 8:  fstp   st0
            0xDD, 0xD8,             // 10: fstp   st0
            0xC3,                   // 12: ret
        ],
        &[(0, C0 | C2 | C3), (2, ALL), (6, ALL)],
    )]
    #[case::overwritten_by_compare(
        &[
            0xD9, 0x01,             // 0: fld    [ecx]   ; <- dead: ftst sets all four
            0xD9, 0xE4,             // 2: ftst
            0xDF, 0xE0,             // 4: fnstsw ax
            0xDD, 0xD8,             // 6: fstp   st0
            0xC3,                   // 8: ret
        ],
        &[(0, NONE), (2, ALL)],
    )]
    #[case::trig_c2_overwritten(
        &[
            0xD9, 0xFF,             // 0: fcos           ; <- C2 dead: fsin sets C1 and C2
            0xD9, 0xFE,             // 2: fsin           ; ret may hand them on
            0xC3,                   // 4: ret
        ],
        &[(0, C0 | C3), (2, ALL)],
    )]
    #[case::compare_unread(
        &[
            0xD9, 0xE4,             // 0: ftst           ; <- dead: fxam sets all four
            0xD9, 0xE5,             // 2: fxam
            0xDF, 0xE0,             // 4: fnstsw ax
            0xC3,                   // 6: ret
        ],
        &[(0, NONE), (2, ALL)],
    )]
    fn run_should_skip_overwritten_writes(
        #[case] code: &[u8],
        #[case] expected: &[(u32, ConditionCodes)],
    ) {
        // Act
        let actual = needed(code);

        // Assert
        assert_eq!(actual, expected);
    }
}
