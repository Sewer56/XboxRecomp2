use super::super::prelude::*;

/// What differs between the low and high word unpacks.
pub(super) struct WordUnpack {
    /// Header helper for the MMX forms, like `X86_PUNPCKLWD`.
    pub mmx_helper: &'static str,

    /// Load for the MMX memory form; PUNPCKLWD reads 32 bits, PUNPCKHWD 64.
    pub mmx_load: &'static str,

    /// SSE2 intrinsic for the XMM forms, like `_mm_unpacklo_epi16`.
    pub xmm_intrinsic: &'static str,
}

/// Interleaves the destination's words with the source's, for every operand form.
pub(super) fn emit(ctx: &mut Emitter<'_>, unpack: &WordUnpack) {
    let operands = ctx.insn.operands();
    let src = &operands[1];
    match register(&operands[0]) {
        Some(dst) if is_mm0_to_7(dst) => emit_mmx(ctx, unpack, mmx_index(dst), src),
        Some(dst) if is_xmm0_to_7(dst) => emit_xmm(ctx, unpack, xmm_index(dst), src),
        _ => ctx.missing_form(),
    }
}

fn emit_mmx(ctx: &mut Emitter<'_>, unpack: &WordUnpack, dst_idx: i64, src: &DecodedOperand) {
    let source = match (register(src), memory(src)) {
        (Some(reg), _) if is_mm0_to_7(reg) => ctx.mmx_reg(mmx_index(reg)),
        (None, Some(_)) => format!("{}({})", unpack.mmx_load, ea(src)),
        _ => return ctx.missing_form(),
    };
    let helper = unpack.mmx_helper;
    let dst = ctx.mmx_reg(dst_idx);
    cpp!(ctx.out, "{dst} = {helper}({dst}, {source});");
}

fn emit_xmm(ctx: &mut Emitter<'_>, unpack: &WordUnpack, dst_idx: i64, src: &DecodedOperand) {
    let source = match (register(src), memory(src)) {
        (Some(reg), _) if is_xmm0_to_7(reg) => {
            format!("_mm_castps_si128(ctx.xmm[{}].reg)", xmm_index(reg))
        }
        (None, Some(_)) => format!(
            "_mm_loadu_si128(reinterpret_cast<const __m128i*>(base + ({})))",
            ea(src)
        ),
        _ => return ctx.missing_form(),
    };
    let intrinsic = unpack.xmm_intrinsic;
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_idx}].reg = _mm_castsi128_ps({intrinsic}(_mm_castps_si128(ctx.xmm[{dst_idx}].reg), {source}));"
    );
}
