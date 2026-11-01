use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let size = dst.size;

    // ROR reg/mem, imm
    if immediate(&operands[1]).is_some() {
        let shift_count = immediate(&operands[1]).unwrap().value as u32;

        if register(dst).is_some() {
            let dst_str = get_x86_reg(register(dst).unwrap());
            cpp!(ctx.out, "X86_ROR{size}({dst_str}, {shift_count}, ctx);");
        } else if memory(dst).is_some() {
            let dst_ea = ea(dst);
            cpp!(
                ctx.out,
                "X86_ROR{size}_MEM({dst_ea}, {shift_count}, ctx, base);"
            );
        } else {
            ctx.missing_form();
        }
    }
    // ROR reg/mem, CL
    else if register(&operands[1]) == Some(Register::CL) {
        if register(dst).is_some() {
            let dst_str = get_x86_reg(register(dst).unwrap());
            cpp!(ctx.out, "X86_ROR{size}({dst_str}, ctx.ecx.u8_lo, ctx);");
        } else if memory(dst).is_some() {
            let dst_ea = ea(dst);
            cpp!(
                ctx.out,
                "X86_ROR{size}_MEM({dst_ea}, ctx.ecx.u8_lo, ctx, base);"
            );
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}
