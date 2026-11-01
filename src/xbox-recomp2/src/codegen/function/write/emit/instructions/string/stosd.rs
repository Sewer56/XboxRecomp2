use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // Use DI and CX without changing the upper register halves.
    let attributes = ctx.insn.instruction().attributes;
    let repeat =
        attributes.intersects(InstructionAttributes::HAS_REP | InstructionAttributes::HAS_REPNE);
    if ctx.insn.instruction().address_width == 16 {
        if repeat {
            cpp!(
                ctx.out,
                "
                {{
                const uint16_t step = (ctx.eflags.rare & X86EFlags::DF) ? -4 : 4;
                uint16_t dst = ctx.edi.u16;
                for (uint16_t count = ctx.ecx.u16; count > 0; --count, dst += step) {{
                    X86_STORE_U32(dst, ctx.eax.u32);
                }}
                ctx.edi.u16 = dst;
                ctx.ecx.u16 = 0;
                }}
            "
            );
        } else {
            cpp!(
                ctx.out,
                "
                X86_STORE_U32(ctx.edi.u16, ctx.eax.u32);
                ctx.edi.u16 += (ctx.eflags.rare & X86EFlags::DF) ? -4 : 4;
            "
            );
        }
        return;
    }
    if ctx.insn.instruction().address_width != 32 {
        ctx.missing_form();
        return;
    }

    // Keep each address in guest width before the store adds the memory base,
    // unless unwrapped_string_addresses is on.
    if repeat {
        let start = super::rep_start(ctx);
        cpp!(
            ctx.out,
            "
            if (ctx.eflags.rare & X86EFlags::DF) {{
                for (uint32_t i = 0; i < ctx.ecx.u32; ++i) {{
                    X86_STORE_U32({start} - i * 4, ctx.eax.u32);
                }}
                ctx.edi.u32 -= ctx.ecx.u32 * 4;
            }} else {{
                for (uint32_t i = 0; i < ctx.ecx.u32; ++i) {{
                    X86_STORE_U32({start} + i * 4, ctx.eax.u32);
                }}
                ctx.edi.u32 += ctx.ecx.u32 * 4;
            }}
            ctx.ecx.u32 = 0;
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            X86_STORE_U32(static_cast<uintptr_t>(ctx.edi.u32), ctx.eax.u32);
            ctx.edi.u32 += (ctx.eflags.rare & X86EFlags::DF) ? -4 : 4;
        "
        );
    }
}
