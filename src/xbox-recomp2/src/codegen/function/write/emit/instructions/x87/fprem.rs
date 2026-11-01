use super::super::prelude::*;
use super::helpers::emit_precision_flag;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(ctx.out, "X87_FPREM({fpu});");
    emit_precision_flag(ctx);
}
