//! Spots the instruction patterns the count treats specially.
//!
//! - [`find_fnsave_frstor_pairs`]: see "`fnsave` and `frstor`" in `x87.md`.
//! - [`ffree_run_empties_stack`]: `ffree`s we can count as pops.
//! - [`virtual_call_returns_float`]: see "Calls to unknown targets" in `x87.md`.

use super::super::Depth;
use super::super::instruction::{DepthChange, ffree_slot, stack_effect};
use crate::codegen::function::model::Op;
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::optimizations::helpers::op_graph::{Edge, OpGraph, OpIndex};
use crate::decode::{Insn, InstructionCategory, Mnemonic, Register, memory, register};
use std::collections::BTreeMap;

/// Most ops we look back from an `fnsave` for the `push` of its base register.
const MAX_OPS_TO_PUSH: usize = 32;
/// Most ops we look ahead from an `fnsave` for its `frstor`.
const MAX_OPS_TO_RESTORE: usize = 16;

// ── `ffree` runs ─────────────────────────────────────────────────────────────

/// Whether the ops right after the `ffree st(slot)` at `op` free `st(slot - 1)`
/// down to `st0`.
///
/// ```text
/// ; #stack is 4      real top    as pops
/// ffree st3          t           t+1     ; frees the bottom: #stack is 3
/// ffree st2          t           t+2
/// ffree st1          t           t+3
/// ffree st0          t           t+4     ; empty either way
/// ```
pub(super) fn ffree_run_empties_stack(graph: &OpGraph, op: OpIndex, slot: u8) -> bool {
    let mut at = op;
    for expected in (0..slot).rev() {
        let next = match next_op_in_straight_line(graph, at) {
            Some(next) => next,
            None => return false,
        };

        let frees = match graph.ops[next] {
            Op::Guest(guest) => ffree_slot(&guest.insn) == Some(expected),
            _ => false,
        };

        if !frees {
            return false;
        }

        at = next;
    }

    true
}

// ── `fnsave` and `frstor` ────────────────────────────────────────────────────

/// Maps each matched `frstor` op to its `fnsave` op.
///
/// A match needs a straight run from `fnsave` to `frstor`:
///
/// - No labels, so nothing jumps in.
/// - One call, which we assume leaves the save area alone.
/// - Same address in both, with the base register kept across the call.
pub(super) fn find_fnsave_frstor_pairs(graph: &OpGraph) -> BTreeMap<OpIndex, OpIndex> {
    let mut fnsave_of = BTreeMap::new();
    for op in 0..graph.len() {
        let save = match insn_of(graph, op) {
            Some(insn) if insn.mnemonic() == Mnemonic::FNSAVE => insn,
            _ => continue,
        };

        if let Some(restore) = find_frstor_for_fnsave(graph, op, save) {
            fnsave_of.insert(restore, op);
        }
    }

    fnsave_of
}

// ── Virtual calls ────────────────────────────────────────────────────────────

/// Whether the code after the virtual call at `op` reads a float it returned.
/// [`None`] if we reach a label, jump, call, `ret` or reset first.
pub(super) fn virtual_call_returns_float(
    graph: &OpGraph,
    op: OpIndex,
    depth: Depth,
) -> Option<bool> {
    let mut at = op;
    let mut pushed: Depth = 0;
    for _ in 0..graph.len() {
        // Follow the single path, up to the next label.
        let next = match graph.edges(at) {
            [Edge::Op(next)] => *next as OpIndex,
            _ => return None,
        };

        if graph.labelled[next] {
            return None;
        }

        // Stop at a jump, call or `ret`.
        let insn = match graph.ops[next] {
            Op::Guest(guest) => &guest.insn,
            Op::MergedCmpJmp(merged) => &merged.compare.insn,
            _ => return None,
        };

        let jumps = matches!(
            insn.instruction().meta.category,
            InstructionCategory::COND_BR | InstructionCategory::UNCOND_BR
        );
        if jumps {
            return None;
        }

        // The first read past what we pushed decides.
        let effect = stack_effect(insn);
        if effect.reads > depth.saturating_add(pushed) {
            return Some(true);
        }

        if effect.reads > pushed {
            return Some(false);
        }

        // Otherwise it only touched our own pushes.
        match effect.change {
            DepthChange::By(by) => pushed = pushed.checked_add_signed(by)?,
            DepthChange::Reset => return None,
        }

        if matches!(graph.ops[next], Op::MergedCmpJmp(_)) {
            return None;
        }

        at = next;
    }

    None
}

fn find_frstor_for_fnsave(graph: &OpGraph, save_op: OpIndex, save: &Insn) -> Option<OpIndex> {
    // Example: the code pushes `ecx` before the call and pops it after.
    //
    // ```text
    // push   ecx
    // fnsave [ecx+0x08]
    // call   f
    // add    esp, 0x10    ; drops f's arguments
    // pop    ecx          ; ecx is back
    // frstor [ecx+0x08]   ; <- same address: a match
    // ```

    // The save area must be `[base + offset]`, with a base other than `esp`.
    let area = memory(&save.operands()[0])?;
    let base = area.base;
    if area.index != Register::NONE || base == Register::ESP {
        return None;
    }

    let base_was_pushed = is_pushed_shortly_before(graph, save_op, base);

    let mut op = save_op;
    let mut called = false;
    let mut popped = false;
    for _ in 0..MAX_OPS_TO_RESTORE {
        op = next_op_in_straight_line(graph, op)?;

        // Exactly one call.
        match graph.ops[op] {
            Op::Call(_) if called => return None,
            Op::Call(_) => {
                called = true;
                continue;
            }
            Op::Guest(_) => {}
            _ => return None,
        }

        let insn = insn_of(graph, op)?;

        // The restore must load the same area, after the call.
        if insn.mnemonic() == Mnemonic::FRSTOR {
            let operand = &insn.operands()[0];
            let same_area =
                memory(operand) == Some(area) && operand.size == save.operands()[0].size;
            let base_kept = popped || is_callee_saved(base);
            return (same_area && base_kept && called).then_some(op);
        }

        // Only the `pop` of our `push` may write the base.
        if RegisterAccess::of(insn).writes(base) {
            let pops_it_back = insn.mnemonic() == Mnemonic::POP && base_was_pushed && called;
            if !pops_it_back {
                return None;
            }

            popped = true;
            continue;
        }

        if !is_allowed_between_pair(insn) {
            return None;
        }
    }

    None
}

/// The op that always runs after `op`, with no label or hook on it.
fn next_op_in_straight_line(graph: &OpGraph, op: OpIndex) -> Option<OpIndex> {
    let next = match graph.edges(op) {
        [Edge::Op(next)] => *next as OpIndex,
        _ => return None,
    };

    let joined = graph.labelled[next] || graph.hook(next).is_some();
    (!joined).then_some(next)
}

/// Whether the code pushes `reg` shortly before `op`, with no write to it since.
fn is_pushed_shortly_before(graph: &OpGraph, op: OpIndex, reg: Register) -> bool {
    // Example:
    //
    // ```text
    // push   ecx
    // sub    esp, 0x10
    // fstp   [esp]
    // fstp   [esp+8]
    // fwait
    // fnsave [ecx+8]
    // ```
    let mut at = op;
    for _ in 0..MAX_OPS_TO_PUSH {
        at = match previous_op_in_straight_line(graph, at) {
            Some(previous) => previous,
            None => return false,
        };

        let insn = match insn_of(graph, at) {
            Some(insn) => insn,
            None => return false,
        };

        if insn.mnemonic() == Mnemonic::PUSH && register(&insn.operands()[0]) == Some(reg) {
            return true;
        }

        if RegisterAccess::of(insn).writes(reg) {
            return false;
        }
    }

    false
}

/// The guest instruction in `op`.
fn insn_of<'a>(graph: &OpGraph<'a>, op: OpIndex) -> Option<&'a Insn> {
    match graph.ops[op] {
        Op::Guest(guest) => Some(&guest.insn),
        Op::Call(call) => Some(&call.guest.insn),
        _ => None,
    }
}

/// Whether MSVC functions keep `reg` across a call.
fn is_callee_saved(reg: Register) -> bool {
    matches!(
        reg,
        Register::EBX | Register::ESI | Register::EDI | Register::EBP
    )
}

/// Only `fwait` and `add esp, N` may sit between an `fnsave` and its `frstor`.
fn is_allowed_between_pair(insn: &Insn) -> bool {
    match insn.mnemonic() {
        Mnemonic::FWAIT => true,
        Mnemonic::ADD => register(&insn.operands()[0]) == Some(Register::ESP),
        _ => false,
    }
}

/// The op that always runs right before `op`, if `op` has no label or hook.
fn previous_op_in_straight_line(graph: &OpGraph, op: OpIndex) -> Option<OpIndex> {
    if op == 0 || graph.labelled[op] || graph.hook(op).is_some() {
        return None;
    }

    let previous = op - 1;
    match graph.edges(previous) {
        [Edge::Op(next)] if *next as OpIndex == op => Some(previous),
        _ => None,
    }
}
