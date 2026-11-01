use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(
        ctx.out,
        "ctx.edx.u32 = (ctx.eax.i32 >> 31) ? 0xFFFFFFFFu : 0x00000000u;"
    );
}
