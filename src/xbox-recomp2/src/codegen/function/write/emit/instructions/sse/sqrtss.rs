use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();

    // The host sqrtss gives x86's default NaN and honours MXCSR.
    let Some(dst_reg) = xmm_register(&operands[0]) else {
        return ctx.missing_form();
    };
    let src = match (xmm_register(&operands[1]), memory(&operands[1])) {
        (Some(src_reg), _) => format!("ctx.xmm[{src_reg}].reg"),
        (None, Some(_)) => format!("_mm_set_ss(X86_LOAD_F32({}))", ea(&operands[1])),
        _ => return ctx.missing_form(),
    };
    // sqrt_ss also sets the guest's IE; fast_sse_status skips it.
    let sqrt = if ctx.config.speed_hacks.runs_fast_sse_status() {
        "_mm_sqrt_ss"
    } else {
        "ctx.sqrt_ss"
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_reg}].reg = _mm_move_ss(ctx.xmm[{dst_reg}].reg, {sqrt}({src}));"
    );
}
