use super::super::prelude::*;
use super::helpers::emit_status_clear;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    // FSCALE - Scale ST(0) by 2^trunc(ST(1))
    let hacks = ctx.config.speed_hacks;
    let template = match (hacks.runs_fast_x87_status(), hacks.runs_fast_fscale()) {
        (false, false) => "",
        (true, false) => "<false>",
        (false, true) => "<true, false>",
        (true, true) => "<false, false>",
    };
    // Intel's manual leaves C0, C2 and C3 undefined after FSCALE, so we keep them.
    emit_status_clear(ctx, ConditionCodes::C1);
    cpp!(ctx.out, "X87_FSCALE{template}({fpu});");
}
