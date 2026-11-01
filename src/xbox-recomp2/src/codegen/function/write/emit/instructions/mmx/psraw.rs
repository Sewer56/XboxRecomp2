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
            // MMX register shift by MMX register; counts above 15 fill with the sign
            let src = ctx.mmx_reg(mmx_index(register(src).unwrap()));
            cpp!(ctx.out, "uint64_t shift_val_{base} = {src};");
            emit_mmx_shift(ctx, &dst);
        } else if immediate(src).is_some() {
            // Immediate shift value
            let count = immediate(src).unwrap().value;
            cpp!(
                ctx.out,
                "uint8_t shift_val_{base} = static_cast<uint8_t>({count});"
            );
            emit_mmx_shift(ctx, &dst);
        } else if memory(src).is_some() {
            // Memory operand (use bottom 64 bits)
            let addr = ea(src);
            cpp!(ctx.out, "uint64_t shift_val_{base} = X86_MMX_LOAD({addr});");
            emit_mmx_shift(ctx, &dst);
        } else {
            ctx.missing_form();
        }
    } else if register(dst).is_some() && is_xmm0_to_7(register(dst).unwrap()) {
        // Check if destination is XMM register
        let dst_idx = xmm_index(register(dst).unwrap());

        if register(src).is_some() && is_xmm0_to_7(register(src).unwrap()) {
            // XMM register to XMM register
            let src_idx = xmm_index(register(src).unwrap());
            cpp!(
                ctx.out,
                "ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps(_mm_sra_epi16(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), _mm_castps_si128(ctx.xmm[{src_idx}].reg)));"
            );
        } else if immediate(src).is_some() {
            // Immediate value
            let count = immediate(src).unwrap().value;
            emit_xmm_shift(ctx, dst_idx, &format!("_mm_set_epi64x(0, {count})"));
        } else if memory(src).is_some() {
            let addr = ea(src);
            let count =
                format!("_mm_loadl_epi64(reinterpret_cast<const __m128i*>(base + ({addr})))");
            emit_xmm_shift(ctx, dst_idx, &count);
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}

fn emit_mmx_shift(ctx: &mut Emitter<'_>, dst: &str) {
    let base = u64::from(ctx.base);
    cpp!(
        ctx.out,
        "
        uint64_t dst_val_{base} = {dst};
        {dst} = X86_PSRAW(shift_val_{base}, dst_val_{base});
    "
    );
}

fn emit_xmm_shift(ctx: &mut Emitter<'_>, dst_idx: i64, count: &str) {
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps(_mm_sra_epi16(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), {count}));"
    );
}
