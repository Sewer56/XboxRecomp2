use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    // Determine operand size and load appropriate integer type
    let (load, int_type) = match operands[0].size {
        16 => ("X86_LOAD_U16", "int16_t"),
        32 => ("X86_LOAD_U32", "int32_t"),
        64 => ("X86_LOAD_U64", "int64_t"),
        _ => return ctx.missing_form(),
    };
    if memory(&operands[0]).is_none() {
        return ctx.missing_form();
    }
    let addr = ctx.ea(&operands[0]);
    let checked = stack_template_argument(ctx);
    // Intel's manual leaves C0, C2 and C3 undefined after FILD, so we keep them.
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~X87FPU::C1;
        {fpu}.push{checked}(static_cast<x87_float>(static_cast<{int_type}>({load}({addr}))));
    "
    );
}
