use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();

    if ctx.insn.instruction().operand_count == 0 {
        cpp!(ctx.out, "X86_FXCH({fpu});");
    } else if let Some(reg) = register(&operands[0]) {
        let index = st_index(reg);
        cpp!(ctx.out, "X86_FXCH({fpu}, {index});");
    }
}
