use super::super::prelude::*;
use super::helpers::{packed_prefix, xmm_register};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst_idx), Some(imm)) = (xmm_register(&operands[0]), immediate(&operands[2])) else {
        return ctx.missing_form();
    };
    let src = &operands[1];
    let value = imm.value;

    if let Some(src_idx) = xmm_register(src) {
        cpp!(ctx.out, "X86_SHUFPS({dst_idx}, {src_idx}, {value});");
    } else if memory(src).is_some() {
        let addr = ctx.ea(src);
        let prefix = packed_prefix(ctx);
        cpp!(
            ctx.out,
            "ctx.xmm[{dst_idx}].reg = _mm_shuffle_ps(ctx.xmm[{dst_idx}].reg, {prefix}LOAD_PS({addr}), {value});"
        );
    } else {
        ctx.missing_form();
    }
}
