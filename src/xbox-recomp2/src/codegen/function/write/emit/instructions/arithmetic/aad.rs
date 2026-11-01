use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let Some(base) = immediate(&ctx.insn.operands()[0]).map(|imm| imm.value as u8) else {
        return ctx.missing_form();
    };

    // Intel's CPU manual says OF, AF and CF are undefined after AAD, so we write 0.
    cpp!(
        ctx.out,
        "
        ctx.eax.u16 = (uint8_t)(ctx.eax.u8_lo + ctx.eax.u8_hi * 0x{base:X});
        ctx.eflags.update_test_flags_U8(ctx.eax.u8_lo);
    "
    );
}
