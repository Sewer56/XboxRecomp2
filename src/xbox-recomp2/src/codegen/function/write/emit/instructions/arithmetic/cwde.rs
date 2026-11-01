use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(ctx.out, "ctx.eax.u32 = (int32_t)(int16_t)ctx.eax.u16;");
}
