//! Works out what calling a function we walked does.

use super::popped_by;
use crate::codegen::optimizations::helpers::function_stack::{CallEffect, FunctionStack};
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::optimizations::helpers::known_callers::direct_call_target;
use crate::codegen::optimizations::helpers::stack_frame::{
    FrameStep, STACK_WORD_BYTES, StackFrame,
};
use crate::config::FunctionAddress;
use crate::decode::{Insn, Mnemonic, OperandAction, Register, memory, register};

/// Where `push ebp` saves the caller's EBP, relative to the starting ESP.
const SAVED_EBP: i32 = -(STACK_WORD_BYTES as i32);

/// Reads the call's effect from a function we walked.
///
/// `calls` gives the effect of each function it calls.
///
/// Returns [`None`] if it has no reachable `ret`, or they pop different amounts.
pub(in crate::codegen) fn from_walk(
    stack: &FunctionStack<'_>,
    calls: impl Fn(FunctionAddress) -> Option<CallEffect>,
) -> Option<CallEffect> {
    // Every `ret` we can reach must pop the same amount.
    let mut popped = None;
    for item in stack.reachable() {
        let Some(ret) = item.return_op else {
            continue;
        };
        let bytes = popped_by(ret)?;
        if let Some(previous) = popped
            && previous != bytes
        {
            return None;
        }
        popped = Some(bytes);
    }
    let popped = popped?;

    let keeps_ebp = never_changes_ebp(stack, calls) || restores_ebp(stack);
    Some(CallEffect { popped, keeps_ebp })
}

/// Whether nothing the function runs writes EBP, including the functions it calls.
fn never_changes_ebp(
    stack: &FunctionStack<'_>,
    calls: impl Fn(FunctionAddress) -> Option<CallEffect>,
) -> bool {
    for item in stack.reachable() {
        if item.call_bytes.is_some() {
            // The function we call must keep EBP too.
            let Some(target) = direct_call_target(item.instruction) else {
                return false;
            };
            let Some(effect) = calls(target) else {
                return false;
            };
            if !effect.keeps_ebp {
                return false;
            }
        } else if RegisterAccess::of(&item.instruction.insn).writes(Register::EBP) {
            return false;
        }
    }
    true
}

/// Whether the function saves EBP with `push ebp`, and puts it back before
/// every `ret`.
fn restores_ebp(stack: &FunctionStack<'_>) -> bool {
    // `push ebp` must come first.
    let first = &stack.instructions[stack.entry].instruction.insn;
    if first.mnemonic() != Mnemonic::PUSH || register(&first.operands()[0]) != Some(Register::EBP) {
        return false;
    }

    // Follow every path.
    let mut pending = vec![(stack.entry, true)];
    let mut visited = vec![[false; 2]; stack.instructions.len()];
    while let Some((index, mut original)) = pending.pop() {
        if visited[index][usize::from(original)] {
            continue;
        }
        visited[index][usize::from(original)] = true;
        let item = &stack.instructions[index];
        let frame = item.frame.expect("the stack walk reached this instruction");
        let insn = &item.instruction.insn;

        // EBP must be back by each `ret`.
        if item.return_op.is_some() {
            if !original {
                return false;
            }
            continue;
        }

        if overwrites_saved_ebp(insn, frame, index == stack.entry) {
            return false;
        }

        // Could write over saved EBP through a pointer.
        if item.call_bytes.is_some() {
            return false;
        }

        // See whether this instruction changes/restores EBP.
        if RegisterAccess::of(insn).writes(Register::EBP) {
            let mut after = frame;
            original = match after.step(insn) {
                Some(FrameStep::SetBase) => false,
                Some(FrameStep::Pop(Register::EBP)) if frame.esp == SAVED_EBP => true,
                Some(FrameStep::Restore)
                    if insn.mnemonic() == Mnemonic::LEAVE
                        && frame.ebp.into_option() == Some(SAVED_EBP) =>
                {
                    true
                }
                _ => return false,
            };
        }
        for next in stack.successors(index) {
            pending.push((next, original));
        }
    }
    true
}

/// Whether `insn` may write over the caller's EBP, saved by `push ebp`.
fn overwrites_saved_ebp(insn: &Insn, frame: StackFrame, is_entry: bool) -> bool {
    if insn.mnemonic() == Mnemonic::PUSH {
        let word = STACK_WORD_BYTES as i32;
        return !is_entry && frame.esp < word && frame.esp > SAVED_EBP;
    }

    let count = usize::from(insn.instruction().operand_count);
    for operand in &insn.operands()[..count] {
        let Some(mem) = memory(operand) else {
            continue;
        };
        if !operand
            .action
            .intersects(OperandAction::WRITE | OperandAction::CONDWRITE)
        {
            continue;
        }
        if !matches!(insn.mnemonic(), Mnemonic::MOV | Mnemonic::MOVSS) {
            return true;
        }
        let Some(start) = frame.address(mem) else {
            return true;
        };
        let width = i32::from(operand.size.div_ceil(8));
        if operand.size == 0 || (start < 0 && start + width > SAVED_EBP) {
            return true;
        }
    }
    false
}
