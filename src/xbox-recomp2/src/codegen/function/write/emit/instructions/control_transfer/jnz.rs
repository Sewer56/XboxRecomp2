use super::super::prelude::*;
use super::helpers::print_conditional_branch;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // For JNZ (Jump if Not Zero)
    if usize::from(ctx.insn.instruction().operand_count) >= 1 {
        print_conditional_branch(ctx, false, "X86_CC_NZ(ctx)");
    }
}
