use super::super::prelude::*;
use super::helpers::{emit_exception_flags, emit_precision_flag, emit_status_clear};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let src = &ctx.insn.operands()[0];
    let (load, int_type) = match src.size {
        16 => ("X86_LOAD_U16", "int16_t"),
        32 => ("X86_LOAD_U32", "int32_t"),
        _ => return ctx.missing_form(),
    };
    if memory(src).is_none() {
        return ctx.missing_form();
    }
    let addr = ctx.ea(src);
    // Intel's manual leaves C0, C2 and C3 undefined after FIADD, so we keep them.
    cpp!(
        ctx.out,
        "
        val_double_0 = {fpu}.st0() + (x87_float)({int_type}){load}({addr});
        {fpu}.st0() = val_double_0;
    "
    );
    emit_status_clear(ctx, ConditionCodes::C1);
    emit_precision_flag(ctx);
    emit_exception_flags(ctx, "val_double_0");
}
