use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();

    let dst = &operands[0];
    let src = &operands[1];
    let base = u64::from(ctx.base);

    // Check if destination is MMX register
    if register(dst).is_some() && is_mm0_to_7(register(dst).unwrap()) {
        let dst = ctx.mmx_reg(mmx_index(register(dst).unwrap()));

        if register(src).is_some() && is_mm0_to_7(register(src).unwrap()) {
            let src = ctx.mmx_reg(mmx_index(register(src).unwrap()));
            cpp!(
                ctx.out,
                "
                uint64_t src_val_{base} = {src};
                uint64_t dst_val_{base} = {dst};
                {dst} = X86_PADDW(src_val_{base}, dst_val_{base});
            "
            );
        } else if memory(src).is_some() {
            let addr = ea(src);
            cpp!(
                ctx.out,
                "
                uint64_t src_val_{base} = X86_MMX_LOAD({addr});
                uint64_t dst_val_{base} = {dst};
                {dst} = X86_PADDW(src_val_{base}, dst_val_{base});
            "
            );
        } else {
            ctx.missing_form();
        }
    } else if register(dst).is_some() && is_xmm0_to_7(register(dst).unwrap()) {
        let dst_idx = xmm_index(register(dst).unwrap());

        if register(src).is_some() && is_xmm0_to_7(register(src).unwrap()) {
            // XMM register to XMM register (using intrinsics)
            let src_idx = xmm_index(register(src).unwrap());
            cpp!(
                ctx.out,
                "ctx.xmm[{dst_idx}].reg = _mm_add_epi16(ctx.xmm[{dst_idx}].reg, ctx.xmm[{src_idx}].reg);"
            );
        } else if memory(src).is_some() {
            let addr = ea(src);
            cpp!(ctx.out, "
                __m128i src_val_{base} = _mm_loadu_si128(reinterpret_cast<const __m128i*>(base + ({addr})));
                ctx.xmm[{dst_idx}].reg = _mm_add_epi16(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), src_val_{base});
                ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps(ctx.xmm[{dst_idx}].reg);
            ");
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}
