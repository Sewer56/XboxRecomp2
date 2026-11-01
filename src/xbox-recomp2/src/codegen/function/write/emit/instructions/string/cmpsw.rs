use super::super::prelude::*;

// The exiting REP iteration still steps ESI, EDI and ECX.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let attributes = ctx.insn.instruction().attributes;
    let base = ctx.base.get();

    if attributes.contains(InstructionAttributes::HAS_REPE) {
        cpp!(
            ctx.out,
            "
            {{
            const uint32_t step_{base} = (ctx.eflags.rare & X86EFlags::DF) ? -2 : 2;
            uint32_t count_{base} = ctx.ecx.u32, src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                uint16_t val1 = X86_LOAD_U16(src_{base});
                uint16_t val2 = X86_LOAD_U16(dst_{base});
                ctx.eflags.update_compare_flags_U16(val1, val2);
                if (!(ctx.eflags.common & X86EFlags::ZF)) {{
                    src_{base} += step_{base};
                    dst_{base} += step_{base};
                    count_{base}--;
                    break;
                }}
            }}
            ctx.esi.u32 = src_{base};
            ctx.edi.u32 = dst_{base};
            ctx.ecx.u32 = count_{base};
            }}
        "
        );
    } else if attributes.contains(InstructionAttributes::HAS_REPNE) {
        cpp!(
            ctx.out,
            "
            {{
            const uint32_t step_{base} = (ctx.eflags.rare & X86EFlags::DF) ? -2 : 2;
            uint32_t count_{base} = ctx.ecx.u32, src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                uint16_t val1 = X86_LOAD_U16(src_{base});
                uint16_t val2 = X86_LOAD_U16(dst_{base});
                ctx.eflags.update_compare_flags_U16(val1, val2);
                if (ctx.eflags.common & X86EFlags::ZF) {{
                    src_{base} += step_{base};
                    dst_{base} += step_{base};
                    count_{base}--;
                    break;
                }}
            }}
            ctx.esi.u32 = src_{base};
            ctx.edi.u32 = dst_{base};
            ctx.ecx.u32 = count_{base};
            }}
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            uint16_t val1 = X86_LOAD_U16(ctx.esi.u32);
            uint16_t val2 = X86_LOAD_U16(ctx.edi.u32);
            ctx.eflags.update_compare_flags_U16(val1, val2);
            ctx.esi.u32 += (ctx.eflags.rare & X86EFlags::DF) ? -2 : 2;
            ctx.edi.u32 += (ctx.eflags.rare & X86EFlags::DF) ? -2 : 2;
        "
        );
    }
}
