use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();

    let dst = &operands[0];
    let src = &operands[1];
    let base = u64::from(ctx.base);

    // Check if destination is MMX register
    if register(dst).is_some() && is_mm0_to_7(register(dst).unwrap()) {
        emit_mmx_form(ctx, mmx_index(register(dst).unwrap()), src, base);
    } else if register(dst).is_some() && is_xmm0_to_7(register(dst).unwrap()) {
        // Check if destination is XMM register
        emit_xmm_form(ctx, xmm_index(register(dst).unwrap()), src, base);
    } else {
        ctx.missing_form();
    }
}

fn emit_mmx_form(ctx: &mut Emitter<'_>, dst_idx: i64, src: &DecodedOperand, base: u64) {
    let dst = ctx.mmx_reg(dst_idx);
    if register(src).is_some() && is_mm0_to_7(register(src).unwrap()) {
        let src = ctx.mmx_reg(mmx_index(register(src).unwrap()));
        cpp!(
            ctx.out,
            "
            __m64 dst_val_{base} = _mm_cvtsi64_m64({dst});
            __m64 src_val_{base} = _mm_cvtsi64_m64({src});
        "
        );
        emit_mmx_pack_result(ctx, &dst, base);
    } else if memory(src).is_some() {
        // Check operand size
        let addr = ctx.ea(src);
        cpp!(
            ctx.out,
            "
            // Pack and saturate doublewords to words (MMX + memory)
            __m64 dst_val_{base} = _mm_cvtsi64_m64({dst});
        "
        );

        if src.size == 32 {
            // 32-bit memory operand (one doubleword)
            cpp!(
                ctx.out,
                "
                // Loading 32 bits (one doubleword)
                uint32_t src_u32_{base} = X86_LOAD_U32({addr});
                __m64 src_val_{base} = _mm_cvtsi32_si64(src_u32_{base});
            "
            );
        } else {
            // 64-bit memory operand (two doublewords)
            cpp!(
                ctx.out,
                "
                __m64 src_val_{base} = _mm_cvtsi64_m64(X86_MMX_LOAD({addr}));
            "
            );
        }

        emit_mmx_pack_result(ctx, &dst, base);
    } else {
        ctx.missing_form();
    }
}

fn emit_xmm_form(ctx: &mut Emitter<'_>, dst_idx: i64, src: &DecodedOperand, base: u64) {
    if register(src).is_some() && is_xmm0_to_7(register(src).unwrap()) {
        // XMM register to XMM register (using intrinsics)
        let src_idx = xmm_index(register(src).unwrap());
        cpp!(ctx.out, "
            ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps(_mm_packs_epi32(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), _mm_castps_si128(ctx.xmm[{src_idx}].reg)));
        ");
    } else if memory(src).is_some() {
        let addr = ctx.ea(src);
        match src.size {
            64 => cpp!(ctx.out, "
                uint64_t src_u64_{base} = X86_LOAD_U64({addr});
                __m128i src_val_{base} = _mm_set_epi64x(0, src_u64_{base});
            "),
            32 => cpp!(ctx.out, "
                uint32_t src_u32_{base} = X86_LOAD_U32({addr});
                __m128i src_val_{base} = _mm_set_epi32(0, 0, 0, src_u32_{base});
            "),
            _ => cpp!(ctx.out, "
                __m128i src_val_{base} = _mm_loadu_si128(reinterpret_cast<const __m128i*>(base + ({addr})));
            "),
        }

        cpp!(ctx.out, "
            ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps(_mm_packs_epi32(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), src_val_{base}));
        ");
    } else {
        ctx.missing_form();
    }
}

fn emit_mmx_pack_result(ctx: &mut Emitter<'_>, dst: &str, base: u64) {
    cpp!(
        ctx.out,
        "
        __m64 result_{base} = _mm_packs_pi32(dst_val_{base}, src_val_{base});
        {dst} = _mm_cvtm64_si64(result_{base});
    "
    );
}
