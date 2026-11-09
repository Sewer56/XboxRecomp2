use super::super::prelude::*;

// The exiting REP iteration still steps ESI, EDI and ECX.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let backward = super::backward(ctx);
    let attributes = ctx.insn.instruction().attributes;
    let base = ctx.base.get();
    let (address, step) = super::rep_types(ctx);

    if attributes.contains(InstructionAttributes::HAS_REPE) {
        cpp!(
            ctx.out,
            "
            {{
            const {step} step_{base} = ({backward}) ? -4 : 4;
            uint32_t count_{base} = ctx.ecx.u32;
            {address} src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                val_uint32_0 = X86_LOAD_U32(src_{base});
                val_uint32_1 = X86_LOAD_U32(dst_{base});
                ctx.eflags.update_compare_flags_U32(val_uint32_0, val_uint32_1);
                if (!(ctx.eflags.common & X86EFlags::ZF)) {{
                    src_{base} += step_{base};
                    dst_{base} += step_{base};
                    count_{base}--;
                    break;
                }}
            }}
            ctx.esi.u32 = static_cast<uint32_t>(src_{base});
            ctx.edi.u32 = static_cast<uint32_t>(dst_{base});
            ctx.ecx.u32 = count_{base};
            }}
        "
        );
    } else if attributes.contains(InstructionAttributes::HAS_REPNE) {
        cpp!(
            ctx.out,
            "
            {{
            const {step} step_{base} = ({backward}) ? -4 : 4;
            uint32_t count_{base} = ctx.ecx.u32;
            {address} src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                val_uint32_0 = X86_LOAD_U32(src_{base});
                val_uint32_1 = X86_LOAD_U32(dst_{base});
                ctx.eflags.update_compare_flags_U32(val_uint32_0, val_uint32_1);
                if (ctx.eflags.common & X86EFlags::ZF) {{
                    src_{base} += step_{base};
                    dst_{base} += step_{base};
                    count_{base}--;
                    break;
                }}
            }}
            ctx.esi.u32 = static_cast<uint32_t>(src_{base});
            ctx.edi.u32 = static_cast<uint32_t>(dst_{base});
            ctx.ecx.u32 = count_{base};
            }}
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            val_uint32_0 = X86_LOAD_U32(ctx.esi.u32);
            val_uint32_1 = X86_LOAD_U32(ctx.edi.u32);
            ctx.eflags.update_compare_flags_U32(val_uint32_0, val_uint32_1);
            ctx.esi.u32 += ({backward}) ? -4 : 4;
            ctx.edi.u32 += ({backward}) ? -4 : 4;
        "
        );
    }
}
