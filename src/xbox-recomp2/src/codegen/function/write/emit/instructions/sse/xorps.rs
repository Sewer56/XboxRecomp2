use super::super::prelude::*;
use super::helpers::{packed_source, xmm_register};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    // XORPS xmm, xmm/[mem]
    let (Some(dst_reg), Some(src)) = (xmm_register(&operands[0]), packed_source(ctx, &operands[1]))
    else {
        return ctx.missing_form();
    };
    let dst = format!("ctx.xmm[{dst_reg}].reg");
    cpp!(ctx.out, "{dst} = _mm_xor_ps({dst}, {src});");
}
