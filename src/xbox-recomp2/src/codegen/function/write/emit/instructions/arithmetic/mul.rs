use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let flags = ctx.flag_template(false);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        return ctx.missing_form();
    }

    let op = &operands[0];
    let size_suffix = operand_size_name(op.size);

    let rhs = if let Some(reg) = register(op) {
        get_x86_reg(reg)
    } else if memory(op).is_some() {
        format!("X86_LOAD_{size_suffix}({})", ctx.ea(op))
    } else {
        return ctx.missing_form();
    };

    let size = op.size;
    cpp!(ctx.out, "X86_MUL{size}{flags}({rhs}, ctx);");
}
