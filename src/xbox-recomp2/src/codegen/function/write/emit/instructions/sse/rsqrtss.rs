use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let Some(dst_reg) = xmm_register(&operands[0]) else {
        return ctx.missing_form();
    };
    let src = match (xmm_register(&operands[1]), memory(&operands[1])) {
        (Some(src_reg), _) => format!("ctx.xmm[{src_reg}].reg"),
        (None, Some(_)) => format!("_mm_set_ss(X86_LOAD_F32({}))", ea(&operands[1])),
        _ => return ctx.missing_form(),
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_reg}].reg = _mm_move_ss(ctx.xmm[{dst_reg}].reg, _mm_rsqrt_ss({src}));"
    );
}
