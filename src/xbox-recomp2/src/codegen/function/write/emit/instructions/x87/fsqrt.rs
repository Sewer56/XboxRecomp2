use super::super::prelude::*;
use super::helpers::status_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let status = status_template_argument(ctx);
    // Intel's manual leaves C0, C2 and C3 undefined after FSQRT, so we keep them.
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~X87FPU::C1;
        X87_FSQRT{status}({fpu});
    "
    );
}
