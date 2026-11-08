use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    if register(dst).is_none() || immediate(src).is_some() {
        return ctx.missing_form();
    }
    let size_suffix = operand_size_name(src.size);

    let dst_str = ctx.format_operand(dst, size_suffix);
    let src_str = ctx.format_operand(src, size_suffix);
    let size = src.size;
    cpp!(ctx.out, "X86_BSF{size}({dst_str}, {src_str}, ctx);");
}
