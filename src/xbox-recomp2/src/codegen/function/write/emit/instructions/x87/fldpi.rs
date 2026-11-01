use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let checked = stack_template_argument(ctx);
    cpp!(ctx.out, "{fpu}.push{checked}(X87FPU::PI);");
}
