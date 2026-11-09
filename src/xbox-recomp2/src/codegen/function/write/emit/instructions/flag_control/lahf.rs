use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(ctx.out, "ctx.eax.u8_hi = ctx.eflags.common | 0x2;");
}
