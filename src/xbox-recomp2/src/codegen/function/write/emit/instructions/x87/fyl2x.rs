use super::super::prelude::*;
use super::helpers::{emit_precision_flag, emit_st0_exception_flags, stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(
        ctx.out,
        "{fpu}.st1() = {fpu}.st1() * std::log2({fpu}.st0());"
    );
    emit_precision_flag(ctx);
    let checked = stack_template_argument(ctx);
    cpp!(ctx.out, "{fpu}.pop{checked}();");
    emit_st0_exception_flags(ctx);
}
