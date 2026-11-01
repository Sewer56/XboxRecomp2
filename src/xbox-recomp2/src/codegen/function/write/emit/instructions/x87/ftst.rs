use super::super::prelude::*;
use super::helpers::status_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let status = status_template_argument(ctx);
    cpp!(ctx.out, "{fpu}.compare_and_pop{status}(0.0, 0);");
}
