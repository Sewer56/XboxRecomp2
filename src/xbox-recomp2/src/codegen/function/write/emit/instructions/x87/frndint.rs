use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    // Round with the rounding mode in the control word
    if ctx.config.speed_hacks.runs_fast_x87_status() {
        cpp!(ctx.out, "{fpu}.st0() = {fpu}.apply_rounding({fpu}.st0());");
    } else {
        cpp!(ctx.out, "{fpu}.frndint();");
    }
}
