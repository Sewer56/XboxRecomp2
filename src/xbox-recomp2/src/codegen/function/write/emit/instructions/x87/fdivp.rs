use super::super::prelude::*;
use super::helpers::status_stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let index = operands
        .first()
        .and_then(register)
        .filter(|reg| reg.class() == RegisterClass::X87)
        .map_or(1, st_index);
    let base = ctx.base.get();
    let status = status_stack_template_argument(ctx);
    cpp!(ctx.out, "X87_FDIVP{status}({fpu}, {index}, 0x{base:X});");
}
