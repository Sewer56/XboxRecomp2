use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        return ctx.missing_form();
    }

    let op = &operands[0];
    let size_suffix = operand_size_name(op.size);

    if register(op).is_some() {
        let op_str = get_x86_reg(register(op).unwrap());
        cpp!(ctx.out, "{op_str} = ~{op_str};");
    } else if memory(op).is_some() {
        let op_ea = ctx.ea(op);
        cpp!(
            ctx.out,
            "X86_STORE_{size_suffix}({op_ea}, ~X86_LOAD_{size_suffix}({op_ea}));"
        );
    } else {
        ctx.missing_form();
    }
}
