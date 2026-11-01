use super::super::prelude::*;
use super::helpers::{emit_precision_flag, emit_st0_exception_flags};

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
    let addr = ea(src);
    cpp!(ctx.out, "{fpu}.st0() *= ({int_type}){load}({addr});");
    emit_precision_flag(ctx);
    emit_st0_exception_flags(ctx);
}
