use super::super::prelude::*;

// The XMM form is SSE2, which the Xbox CPU doesn't have.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let Some(dst) = register(&operands[0]).filter(|reg| is_mm0_to_7(*reg)) else {
        return ctx.missing_form();
    };
    let src = &operands[1];
    let source = match (register(src), memory(src)) {
        // PAVGB mm1, mm2 (MMX ← MMX)
        (Some(reg), _) if is_mm0_to_7(reg) => ctx.mmx_reg(mmx_index(reg)),
        (None, Some(_)) => format!("X86_MMX_LOAD({})", ea(src)),
        _ => return ctx.missing_form(),
    };

    let dst = ctx.mmx_reg(mmx_index(dst));
    let base = u64::from(ctx.base);
    cpp!(
        ctx.out,
        "
        uint64_t dst_val_{base} = {dst};
        uint64_t src_val_{base} = {source};
        {dst} = X86_PAVGB(dst_val_{base}, src_val_{base});
    "
    );
}
