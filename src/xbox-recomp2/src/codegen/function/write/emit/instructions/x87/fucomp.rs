use super::super::prelude::*;
use super::helpers::status_stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    // Unordered Compare and Pop Once
    let operands = ctx.insn.operands();
    let index = operands
        .get(1)
        .and_then(register)
        .filter(|reg| reg.class() == RegisterClass::X87)
        .map_or(1, st_index);
    let status = status_stack_template_argument(ctx);
    cpp!(
        ctx.out,
        "{fpu}.compare_and_pop{status}({fpu}.st{index}(), 1, false);"
    );
}
