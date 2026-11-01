use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let checked = stack_template_argument(ctx);
    // No operands, always pushes 1.0 onto ST(0)
    // Intel's manual leaves C0, C2 and C3 undefined after FLD1, so we keep them.
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~X87FPU::C1;
        {fpu}.push{checked}(X87FPU::ONE);
    "
    );
}
