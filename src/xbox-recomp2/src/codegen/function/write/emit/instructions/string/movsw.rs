use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let base = ctx.base.get();
    let (address, step) = super::rep_types(ctx);

    if ctx
        .insn
        .instruction()
        .attributes
        .contains(InstructionAttributes::HAS_REP)
    {
        cpp!(
            ctx.out,
            "
            {{
            const {step} step_{base} = (ctx.eflags.rare & X86EFlags::DF) ? -2 : 2;
            {address} src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            val_uint32_0 = ctx.ecx.u32;
            for (; val_uint32_0 > 0; val_uint32_0--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                X86_STORE_U16(dst_{base}, X86_LOAD_U16(src_{base}));
            }}
            ctx.esi.u32 = static_cast<uint32_t>(src_{base});
            ctx.edi.u32 = static_cast<uint32_t>(dst_{base});
            ctx.ecx.u32 = 0;
            }}
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            X86_STORE_U16(ctx.edi.u32, X86_LOAD_U16(ctx.esi.u32));
            ctx.eflags.update_string_pointer_word(ctx.esi.u32);
            ctx.eflags.update_string_pointer_word(ctx.edi.u32);
        "
        );
    }
}
