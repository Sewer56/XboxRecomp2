use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let Some(src) = register(&operands[1]) else {
        return ctx.missing_form();
    };
    let size = dst.size;

    if memory(dst).is_some() {
        let dst_ea = ea(dst);
        let src_str = get_x86_reg(src);
        cpp!(
            ctx.out,
            "X86_CMPXCHG{size}_MEM({dst_ea}, {src_str}, ctx, base);"
        );
    } else if register(dst).is_some() {
        let dst_str = get_x86_reg(register(dst).unwrap());
        let src_str = get_x86_reg(src);
        cpp!(ctx.out, "X86_CMPXCHG{size}({dst_str}, {src_str}, ctx);");
    } else {
        ctx.missing_form();
    }
}
