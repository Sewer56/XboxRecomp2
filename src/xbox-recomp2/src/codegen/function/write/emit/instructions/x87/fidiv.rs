use super::super::prelude::*;
use super::helpers::{emit_precision_flag, emit_st0_exception_flags, emit_status_clear};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let src = &ctx.insn.operands()[0];
    let (helper, load, int_type) = match src.size {
        16 => ("X87_FIDIV_16", "X86_LOAD_U16", "int16_t"),
        32 => ("X87_FIDIV_32", "X86_LOAD_U32", "int32_t"),
        _ => return ctx.missing_form(),
    };
    if memory(src).is_none() {
        return ctx.missing_form();
    }
    let addr = ctx.ea(src);
    let base = ctx.base.get();
    cpp!(
        ctx.out,
        "{fpu}.st0() = (x87_float){helper}({fpu}.st0(), ({int_type}){load}({addr}), 0x{base:X});"
    );
    // Intel's manual leaves C0, C2 and C3 undefined after FIDIV, so we keep them.
    emit_status_clear(ctx, ConditionCodes::C1);
    emit_precision_flag(ctx);
    emit_st0_exception_flags(ctx);
}
