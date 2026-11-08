use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let src = &operands[1];
    match register(&operands[0]) {
        Some(dst) if is_mm0_to_7(dst) => emit_mmx(ctx, mmx_index(dst), src),
        Some(dst) if is_xmm0_to_7(dst) => emit_xmm(ctx, xmm_index(dst), src),
        _ => ctx.missing_form(),
    }
}

fn emit_mmx(ctx: &mut Emitter<'_>, dst_idx: i64, src: &DecodedOperand) {
    let dst = ctx.mmx_reg(dst_idx);

    // Immediate counts shift directly.
    if let Some(count) = immediate(src) {
        // Counts above 63 clear the register.
        if count.value > 63 {
            cpp!(ctx.out, "{dst} = 0;");
        } else {
            cpp!(ctx.out, "{dst} >>= {};", count.value);
        }
        return;
    }

    // Register and memory counts use all 64 bits.
    let count = match (register(src), memory(src)) {
        (Some(reg), _) if is_mm0_to_7(reg) => ctx.mmx_reg(mmx_index(reg)),
        (None, Some(_)) => format!("X86_LOAD_U64({})", ctx.ea(src)),
        _ => return ctx.missing_form(),
    };
    cpp!(ctx.out, "{dst} = X86_PSRLQ({dst}, {count});");
}

fn emit_xmm(ctx: &mut Emitter<'_>, dst_idx: i64, src: &DecodedOperand) {
    let dst = format!("_mm_castps_si128(ctx.xmm[{dst_idx}].reg)");
    let shifted = match (register(src), memory(src), immediate(src)) {
        (_, _, Some(count)) => format!("_mm_srli_epi64({dst}, {})", count.value),
        (Some(reg), _, _) if is_xmm0_to_7(reg) => format!(
            "_mm_srl_epi64({dst}, _mm_castps_si128(ctx.xmm[{}].reg))",
            xmm_index(reg)
        ),
        // The count is the low 64 bits, but the instruction reads all 128.
        (None, Some(_), None) => format!(
            "_mm_srl_epi64({dst}, _mm_loadu_si128(reinterpret_cast<const __m128i*>(base + ({}))))",
            ctx.ea(src)
        ),
        _ => return ctx.missing_form(),
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps({shifted});"
    );
}
