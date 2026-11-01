use super::super::prelude::*;
use super::helpers::{packed_source, xmm_register};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst_reg), Some(src)) = (xmm_register(&operands[0]), packed_source(ctx, &operands[1]))
    else {
        return ctx.missing_form();
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_reg}].reg = _mm_and_ps(ctx.xmm[{dst_reg}].reg, {src});"
    );
}
