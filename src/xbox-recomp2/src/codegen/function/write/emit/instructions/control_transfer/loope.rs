use super::super::prelude::*;
use super::helpers::print_conditional_branch;

// The count goes down without touching the flags.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    if ctx.insn.instruction().address_width == 16 {
        cpp!(ctx.out, "ctx.ecx.u16 -= 1;");
        print_conditional_branch(ctx, false, "ctx.ecx.u16 != 0 && X86_CC_Z(ctx)");
    } else {
        cpp!(ctx.out, "ctx.ecx.u32 -= 1;");
        print_conditional_branch(ctx, false, "ctx.ecx.u32 != 0 && X86_CC_Z(ctx)");
    }
}
