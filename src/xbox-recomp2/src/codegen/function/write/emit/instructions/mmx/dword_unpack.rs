use super::super::prelude::*;

// The XMM forms are SSE2, which the Xbox CPU doesn't have.
pub(super) fn emit(ctx: &mut Emitter<'_>, load: &str, result: &str) {
    let operands = ctx.insn.operands();
    let Some(dst) = register(&operands[0]).filter(|reg| is_mm0_to_7(*reg)) else {
        return ctx.missing_form();
    };
    let src = &operands[1];
    let source = match (register(src), memory(src)) {
        (Some(reg), _) if is_mm0_to_7(reg) => ctx.mmx_reg(mmx_index(reg)),
        (None, Some(_)) => format!("{load}({})", ea(src)),
        _ => return ctx.missing_form(),
    };

    let dst = ctx.mmx_reg(mmx_index(dst));
    cpp!(
        ctx.out,
        "
        val_uint64_0 = {dst};
        val_uint64_1 = {source};
        {dst} = {result};
    "
    );
}
