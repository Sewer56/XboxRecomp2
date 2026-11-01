use super::super::prelude::*;

// Subtracts from AX so a borrow out of AL reaches AH.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // Intel's CPU manual says OF, SF, ZF and PF are undefined after AAS,
    // so we write 0.
    cpp!(
        ctx.out,
        "
        {{
            const uint32_t adjust = uint32_t((ctx.eax.u8_lo & 0x0F) > 9) | ((ctx.eflags.common & X86EFlags::AF) >> 4);
            ctx.eax.u32 = (ctx.eax.u32 & 0xFFFF0000u) | ((ctx.eax.u32 - adjust * 0x106) & 0xFF0F);
            ctx.eflags.common = adjust * (X86EFlags::AF | X86EFlags::CF);
        }}
    "
    );
}
