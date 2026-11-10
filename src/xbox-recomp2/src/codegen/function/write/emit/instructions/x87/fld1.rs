use super::super::prelude::*;
use super::helpers::{emit_status_clear, stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let checked = stack_template_argument(ctx);
    // No operands, always pushes 1.0 onto ST(0)
    // Intel's manual leaves C0, C2 and C3 undefined after FLD1, so we keep them.
    emit_status_clear(ctx, ConditionCodes::C1);
    cpp!(ctx.out, "{fpu}.push{checked}(X87FPU::ONE);");
}
