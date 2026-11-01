use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let base = ctx.base.get();

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
            const uint32_t step_{base} = (ctx.eflags.rare & X86EFlags::DF) ? -1 : 1;
            uint32_t count_{base} = ctx.ecx.u32, src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                X86_STORE_U8(dst_{base}, X86_LOAD_U8(src_{base}));
            }}
            ctx.esi.u32 = src_{base};
            ctx.edi.u32 = dst_{base};
            ctx.ecx.u32 = 0;
            }}
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            X86_STORE_U8(ctx.edi.u32, X86_LOAD_U8(ctx.esi.u32));
            ctx.eflags.update_string_pointer(ctx.esi.u32);
            ctx.eflags.update_string_pointer(ctx.edi.u32);
        "
        );
    }
}
