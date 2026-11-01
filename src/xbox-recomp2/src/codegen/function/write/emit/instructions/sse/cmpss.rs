use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst_reg), Some(imm)) = (xmm_register(&operands[0]), immediate(&operands[2])) else {
        return ctx.missing_form();
    };

    // Only the low three bits pick the predicate on SSE1.
    let cmp_func = match imm.value & 0x7 {
        0 => "_mm_cmpeq_ss",    // Equal
        1 => "_mm_cmplt_ss",    // Less than
        2 => "_mm_cmple_ss",    // Less than or equal
        3 => "_mm_cmpunord_ss", // Unordered
        4 => "_mm_cmpneq_ss",   // Not equal
        5 => "_mm_cmpnlt_ss",   // Not less than
        6 => "_mm_cmpnle_ss",   // Not less than or equal
        _ => "_mm_cmpord_ss",   // Ordered
    };

    let src = &operands[1];
    let source = match (xmm_register(src), memory(src)) {
        (Some(src_reg), _) => format!("ctx.xmm[{src_reg}].reg"),
        (None, Some(_)) => format!("_mm_set_ss(X86_LOAD_F32({}))", ea(src)),
        _ => return ctx.missing_form(),
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_reg}].reg = {cmp_func}(ctx.xmm[{dst_reg}].reg, {source});"
    );
}
