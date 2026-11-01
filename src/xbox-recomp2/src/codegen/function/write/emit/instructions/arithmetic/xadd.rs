use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let size = src.size;
    let Some(src_reg) = register(src) else {
        return ctx.missing_form();
    };
    let src_str = get_x86_reg(src_reg);

    if memory(dst).is_some() {
        let dst_ea = ea(dst);
        if ctx
            .insn
            .instruction()
            .attributes
            .contains(InstructionAttributes::HAS_LOCK)
        {
            cpp!(
                ctx.out,
                "X86_XADD{size}_ATOMIC({dst_ea}, {src_str}, ctx, base);"
            );
        } else {
            cpp!(
                ctx.out,
                "X86_XADD{size}_MEM({dst_ea}, {src_str}, ctx, base);"
            );
        }
    } else if let Some(dst_reg) = register(dst) {
        let dst_str = get_x86_reg(dst_reg);
        cpp!(ctx.out, "X86_XADD{size}({dst_str}, {src_str}, ctx);");
    } else {
        ctx.missing_form();
    }
}
