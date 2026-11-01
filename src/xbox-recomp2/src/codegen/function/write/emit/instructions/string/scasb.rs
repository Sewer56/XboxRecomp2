use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let attributes = ctx.insn.instruction().attributes;
    let base = ctx.base.get();

    if attributes.contains(InstructionAttributes::HAS_REPE) {
        cpp!(
            ctx.out,
            "
            {{
            const uint32_t step_{base} = (ctx.eflags.rare & X86EFlags::DF) ? -1 : 1;
            uint32_t count_{base} = ctx.ecx.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, dst_{base} += step_{base}) {{
                uint8_t val = X86_LOAD_U8(dst_{base});
                ctx.eflags.update_compare_flags_U8(ctx.eax.u8_lo, val);
                if (!(ctx.eflags.common & X86EFlags::ZF)) {{
                    dst_{base} += step_{base};
                    count_{base}--;
                    break;
                }}
            }}
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
            const uint8_t al_val_{base} = ctx.eax.u8_lo;
            const bool df_{base} = (ctx.eflags.rare & X86EFlags::DF) != 0;
            uint32_t count_{base} = ctx.ecx.u32;
            while (count_{base} > 0) {{
                ctx.eflags.update_compare_flags_U8(al_val_{base}, X86_LOAD_U8(ctx.edi.u32));
                ctx.edi.u32 += df_{base} ? -1 : 1;
                count_{base}--;
                if ((ctx.eflags.common & X86EFlags::ZF) != 0) {{
                    break;
                }}
            }}
            ctx.ecx.u32 = count_{base};
            }}
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            uint8_t mem_val = X86_LOAD_U8(ctx.edi.u32);
            ctx.eflags.update_compare_flags_U8(ctx.eax.u8_lo, mem_val);
            ctx.edi.u32 += (ctx.eflags.rare & X86EFlags::DF) ? -1 : 1;
        "
        );
    }
}
