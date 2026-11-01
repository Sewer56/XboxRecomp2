use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    // PEXTRW requires immediate operand for index
    let (Some(dst), Some(src), Some(imm)) = (
        register(&operands[0]),
        register(&operands[1]),
        immediate(&operands[2]),
    ) else {
        return ctx.missing_form();
    };
    let dst_reg = get_x86_reg(dst);

    if is_mm0_to_7(src) {
        let word_idx = imm.value & 0x3;
        let src = ctx.mmx_reg(mmx_index(src));
        cpp!(ctx.out, "{dst_reg} = X86_PEXTRW<{word_idx}>({src});");
    } else if is_xmm0_to_7(src) {
        let word_idx = imm.value & 0x7;
        let src_idx = xmm_index(src);
        cpp!(
            ctx.out,
            "{dst_reg} = (XMM_U32({src_idx}, {word_idx} / 2) >> (({word_idx}  % 2) * 16)) & 0xFFFF;"
        );
    } else {
        ctx.missing_form();
    }
}
