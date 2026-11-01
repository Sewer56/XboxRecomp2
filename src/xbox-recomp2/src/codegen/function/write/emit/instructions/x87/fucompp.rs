use super::super::prelude::*;
use super::helpers::status_stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    // Unordered Compare and Pop Twice
    let status = status_stack_template_argument(ctx);
    cpp!(
        ctx.out,
        "{fpu}.compare_and_pop{status}({fpu}.st1(), 2, false);"
    );
}
