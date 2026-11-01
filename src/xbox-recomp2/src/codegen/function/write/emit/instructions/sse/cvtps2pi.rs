use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    // The destination must be an MMX register
    let Some(dst_idx) = register(&operands[0])
        .filter(|reg| is_mm0_to_7(*reg))
        .map(mmx_index)
    else {
        return ctx.missing_form();
    };
    let src = &operands[1];
    let dst = ctx.mmx_reg(dst_idx);

    if let Some(src_idx) = xmm_register(src) {
        // XMM register to MMX register
        cpp!(ctx.out, "{dst} = X86_CVTPS2PI(ctx.xmm[{src_idx}].reg);");
    } else if memory(src).is_some() {
        // The memory form reads 64 bits: two floats.
        let addr = ea(src);
        let base = ctx.base.get();
        cpp!(
            ctx.out,
            "
            __m128 src_val_{base} = _mm_castsi128_ps(_mm_cvtsi64_si128(static_cast<long long>(X86_LOAD_U64({addr}))));
            {dst} = X86_CVTPS2PI(src_val_{base});
        "
        );
    } else {
        ctx.missing_form();
    }
}
