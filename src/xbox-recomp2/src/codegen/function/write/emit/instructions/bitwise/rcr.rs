use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let size = dst.size;

    let shift_expr = if let Some(imm) = immediate(&operands[1]) {
        imm.value.to_string()
    } else if register(&operands[1]) == Some(Register::CL) {
        "ctx.ecx.u8_lo".to_string()
    } else {
        return ctx.missing_form();
    };

    if register(dst).is_some() {
        let dst_str = get_x86_reg(register(dst).unwrap());
        cpp!(
            ctx.out,
            "{dst_str} = X86_RCR{size}({dst_str}, {shift_expr}, ctx.eflags);"
        );
    } else if memory(dst).is_some() {
        let dst_ea = ctx.ea(dst);
        cpp!(
            ctx.out,
            "X86_RCR{size}_MEM({dst_ea}, {shift_expr}, ctx, base);"
        );
    } else {
        ctx.missing_form();
    }
}
