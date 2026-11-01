use super::super::prelude::*;

// Adds to AX so a carry out of AL reaches AH.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // Intel's CPU manual says OF, SF, ZF and PF are undefined after AAA,
    // so we write 0.
    cpp!(
        ctx.out,
        "
        if (((ctx.eax.u8_lo & 0x0F) > 9) || (ctx.eflags.common & X86EFlags::AF)) {{
            ctx.eax.u16 += 0x106;
            ctx.eflags.common = X86EFlags::AF | X86EFlags::CF;
        }} else {{
            ctx.eflags.common = 0;
        }}
        ctx.eax.u8_lo &= 0x0F;
    "
    );
}
