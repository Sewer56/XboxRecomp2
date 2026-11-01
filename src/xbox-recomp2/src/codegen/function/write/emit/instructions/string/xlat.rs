use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(
        ctx.out,
        "ctx.eax.u8_lo = X86_LOAD_U8(ctx.ebx.u32 + ctx.eax.u8_lo);"
    );
}
