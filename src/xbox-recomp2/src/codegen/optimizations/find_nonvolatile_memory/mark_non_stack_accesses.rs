//! Makes the accesses outside the stack nonvolatile, where it's safe.

use crate::codegen::function::model::FunctionModel;
use crate::codegen::optimizations::helpers::instruction::stack_slot::is_stack_slot;
use crate::decode::{Insn, OperandAction, memory};

/// Makes the other accesses nonvolatile, except:
///
/// - The reads a loop might wait on, from `waiting_reads`.
/// - Stores outside the stack, if the function has such a read.
pub(super) fn mark_non_stack_accesses(model: &mut FunctionModel, waiting_reads: &[bool]) {
    let function_waits = waiting_reads.contains(&true);
    let ebp_frame = model.ebp_frame;
    let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);

    for (index, op) in ops.enumerate() {
        if waiting_reads.get(index).copied().unwrap_or(false) {
            continue;
        }

        for guest in op.guests_mut() {
            if function_waits && writes_outside_stack(&guest.insn, ebp_frame) {
                continue;
            }

            guest.nonvolatile = true;
        }
    }
}

/// Whether `insn` writes memory outside the stack.
fn writes_outside_stack(insn: &Insn, ebp_frame: bool) -> bool {
    let count = usize::from(insn.instruction().operand_count);
    for operand in &insn.operands()[..count] {
        let writes = operand
            .action
            .intersects(OperandAction::WRITE | OperandAction::CONDWRITE);

        if !writes {
            continue;
        }

        match memory(operand) {
            Some(mem) if !is_stack_slot(mem, ebp_frame) => return true,
            _ => {}
        }
    }

    false
}
