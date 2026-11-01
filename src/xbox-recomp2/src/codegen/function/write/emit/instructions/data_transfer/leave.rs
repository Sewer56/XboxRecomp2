use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    //Equivalent to: mov esp,ebp; pop ebp
    if ctx.insn.instruction().operand_width == 16 {
        cpp!(
            ctx.out,
            "
            ctx.esp.u32 = ctx.ebp.u32;
            ctx.ebp.u16 = X86_POP16(ctx, base);
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            ctx.esp.u32 = ctx.ebp.u32;
            ctx.ebp.u32 = X86_POP(ctx, base);
        "
        );
    }
}
