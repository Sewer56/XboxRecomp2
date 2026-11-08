//! Works out what calling a function we couldn't walk does.

use super::popped_by;
use crate::codegen::function::model::{Entry, FunctionModel, Op};
use crate::codegen::optimizations::helpers::function_stack::CallEffect;
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::optimizations::helpers::stack_frame::StackFrame;
use crate::decode::{Insn, Mnemonic, Register, register};

/// Trusts the `ret N` of a function we couldn't walk.
///
/// Returns [`None`] if any of these is true:
/// - Function has hooks.
/// - Instruction failed to decode.
/// - Instruction has 16-bit address.
/// - Instruction changes ESP in a way we can't follow.
/// - No `ret`.
/// - Multiple `ret`s pop different amounts.
pub(in crate::codegen) fn trusting_ret_n(model: &FunctionModel) -> Option<CallEffect> {
    if !model.can_change_abi() {
        return None;
    }
    let framed = starts_with_frame(model);

    // Check every instruction./
    let mut popped = None;
    let mut keeps_ebp = true;
    for op in ops(model) {
        let guest = match op {
            Op::Guest(guest) => guest,
            Op::CdqIdiv(idiv) => &idiv.guest,
            Op::Call(call) => {
                // No idea what it does to ebp.
                keeps_ebp = false;
                &call.guest
            }
            Op::Return(ret) => {
                // Every `ret` must pop the same amount.
                let bytes = popped_by(ret)?;
                if let Some(previous) = popped
                    && previous != bytes
                {
                    return None;
                }
                popped = Some(bytes);
                &ret.guest
            }
            Op::MergedCmpJmp(_) | Op::Failed { .. } => return None,
        };
        let insn = &guest.insn;
        let access = RegisterAccess::of(insn);

        // ebp is written, we can't keep it
        if access.writes(Register::EBP) {
            keeps_ebp = false;
        }

        // not 32-bit addressing
        if insn.instruction().address_width != 32 {
            return None;
        }

        // ESP is written in a way we can't follow, bail out
        if access.writes(Register::ESP) && !can_follow_esp_write(insn, framed) {
            return None;
        }
    }
    let popped = popped?;

    Some(CallEffect { popped, keeps_ebp })
}

/// Whether we can follow how `insn` changes ESP.
///
/// `leave` and `mov esp, ebp` are only allowed if the function uses
/// a `frame`.
///
/// i.e. starts with `push ebp; mov ebp, esp`.
/// You've seen that in IDA before, right buddy?
fn can_follow_esp_write(insn: &Insn, framed: bool) -> bool {
    let is_32_bit = insn.instruction().operand_width == 32;
    match insn.mnemonic() {
        // `pop esp`
        Mnemonic::POP if register(&insn.operands()[0]) == Some(Register::ESP) => false,
        Mnemonic::PUSH | Mnemonic::POP | Mnemonic::CALL | Mnemonic::RET => is_32_bit,
        Mnemonic::LEAVE => framed && is_32_bit,
        Mnemonic::MOV => {
            framed
                && register(&insn.operands()[0]) == Some(Register::ESP)
                && register(&insn.operands()[1]) == Some(Register::EBP)
        }
        _ => StackFrame::adjustment(insn).is_some(),
    }
}

/// Whether the function starts with `push ebp; mov ebp, esp`.
fn starts_with_frame(model: &FunctionModel) -> bool {
    if model.entry != Entry::First {
        return false;
    }
    let mut ops = ops(model);
    let Some(Op::Guest(first)) = ops.next() else {
        return false;
    };
    let Some(Op::Guest(second)) = ops.next() else {
        return false;
    };
    let first = &first.insn;
    let second = &second.insn;

    // push ebp
    let is_push = first.mnemonic() == Mnemonic::PUSH;
    let is_32_bit = first.instruction().operand_width == 32;
    let pushes_ebp = register(&first.operands()[0]) == Some(Register::EBP);
    let push_ebp = is_push && is_32_bit && pushes_ebp;

    // mov ebp, esp
    let is_mov = second.mnemonic() == Mnemonic::MOV;
    let to_ebp = register(&second.operands()[0]) == Some(Register::EBP);
    let from_esp = register(&second.operands()[1]) == Some(Register::ESP);
    let mov_ebp_esp = is_mov && to_ebp && from_esp;

    push_ebp && mov_ebp_esp
}

/// Returns every operation in the function, in the order they're written.
fn ops(model: &FunctionModel) -> impl Iterator<Item = &Op> {
    model.blocks.iter().flat_map(|block| &block.ops)
}
