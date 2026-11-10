use super::super::prelude::*;
use super::helpers::{compare_unread, emit_pops, status_stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    if compare_unread(ctx) {
        return emit_pops(ctx, 2);
    }

    let fpu = ctx.fpu();
    // Not handle_fcom: Zydis lists the implicit ST(0) as operand 0.
    let status = status_stack_template_argument(ctx);
    cpp!(ctx.out, "{fpu}.compare_and_pop{status}({fpu}.st1(), 2);");
}
