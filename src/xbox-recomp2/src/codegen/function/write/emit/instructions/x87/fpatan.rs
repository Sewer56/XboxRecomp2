use super::super::prelude::*;
use super::helpers::status_stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let template = status_stack_template_argument(ctx);
    cpp!(ctx.out, "X86_FPATAN{template}({fpu});");
}
