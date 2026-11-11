//! Makes the stack accesses nonvolatile.

use crate::codegen::function::model::{FunctionModel, Op};
use crate::codegen::optimizations::helpers::instruction::stack_slot::is_stack_slot;
use crate::decode::{Insn, OperandVisibility, memory};

/// Makes the stack accesses nonvolatile.
pub(super) fn mark_stack_accesses(model: &mut FunctionModel) {
    let ebp_frame = model.ebp_frame;
    let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);
    for guest in ops.flat_map(Op::guests_mut) {
        if is_stack_access(&guest.insn, ebp_frame) {
            guest.nonvolatile = true;
        }
    }
}

/// Whether `insn` accesses `[esp+N]`, or `[ebp+N]` with `ebp_frame`.
///
/// # Remarks
///
/// Only visible operands count, so `push [ebx]` isn't a stack access.
pub(super) fn is_stack_access(insn: &Insn, ebp_frame: bool) -> bool {
    let count = usize::from(insn.instruction().operand_count);
    insn.operands()[..count]
        .iter()
        .filter(|operand| operand.visibility != OperandVisibility::HIDDEN)
        .filter_map(memory)
        .any(|mem| is_stack_slot(mem, ebp_frame))
}
