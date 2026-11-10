use super::super::prelude::*;
use super::helpers::emit_status_clear;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(ctx.out, "{fpu}.st0() = -{fpu}.st0();");
    emit_status_clear(ctx, ConditionCodes::C1);
}
