use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let (store, int_type) = match operands[0].size {
        16 => ("X86_STORE_U16", "int16_t"),
        32 => ("X86_STORE_U32", "int32_t"),
        64 => ("X86_STORE_U64", "int64_t"),
        _ => return ctx.missing_form(),
    };
    if memory(&operands[0]).is_none() {
        return ctx.missing_form();
    }
    let addr = ctx.ea(&operands[0]);
    let status = if ctx.config.speed_hacks.runs_fast_x87_status() {
        ", false"
    } else {
        ""
    };
    let checked = stack_template_argument(ctx);
    cpp!(
        ctx.out,
        "
        {store}({addr}, ({fpu}.fist<{int_type}{status}>()));
        {fpu}.pop{checked}();
    "
    );
}
