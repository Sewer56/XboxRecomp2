use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        return ctx.missing_form();
    }

    let op = &operands[0];
    let size_suffix = operand_size_name(op.size);

    if let Some(reg) = register(op) {
        let helper = format!("X86_NEG_{size_suffix}");
        super::write_register_op(ctx, reg, &helper, "");
    } else if memory(op).is_some() {
        let op_ea = ctx.ea(op);
        cpp!(
            ctx.out,
            "X86_NEG_{size_suffix}_MEM{memory_flags}({op_ea}, ctx, base);"
        );
    } else {
        ctx.missing_form();
    }
}
