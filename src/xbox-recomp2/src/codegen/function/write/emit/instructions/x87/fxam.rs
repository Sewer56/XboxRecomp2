use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(ctx.out, "X87_FXAM({fpu}, {fpu}.top);");
}
