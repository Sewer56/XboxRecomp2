use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let flags = ctx.flag_template(false);
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 3 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let size = dst.size;

    // Count is masked to 5 bits, 16-bit too; 0 is a no-op.
    let count_expr = if let Some(imm) = immediate(&operands[2]) {
        // SHRD reg/mem, reg, imm
        let count = imm.value & 31;
        if count == 0 {
            return;
        }
        count.to_string()
    } else if register(&operands[2]) == Some(Register::CL) {
        // SHRD reg/mem, reg, CL
        "ctx.ecx.u8_lo & 31".to_string()
    } else {
        return ctx.missing_form();
    };
    let Some(src) = register(src) else {
        return ctx.missing_form();
    };

    if register(dst).is_some() {
        let dst_str = get_x86_reg(register(dst).unwrap());
        let src_str = get_x86_reg(src);
        cpp!(
            ctx.out,
            "X86_SHRD{size}{flags}({dst_str}, {src_str}, {count_expr}, ctx);"
        );
    } else if memory(dst).is_some() {
        let dst_ea = ctx.ea(dst);
        let src_str = get_x86_reg(src);
        cpp!(
            ctx.out,
            "X86_SHRD{size}_MEM{memory_flags}({dst_ea}, {src_str}, {count_expr}, ctx, base);"
        );
    } else {
        ctx.missing_form();
    }
}
