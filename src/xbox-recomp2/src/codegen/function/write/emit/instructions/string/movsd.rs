use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let backward = super::backward(ctx);
    let base = ctx.base.get();
    let (address, step) = super::rep_types(ctx);

    if ctx
        .insn
        .instruction()
        .attributes
        .contains(InstructionAttributes::HAS_REP)
    {
        super::try_memmove(ctx, "uint32_t");
        cpp!(
            ctx.out,
            "
            {{
            const {step} step_{base} = ({backward}) ? -4 : 4;
            uint32_t count_{base} = ctx.ecx.u32;
            {address} src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                X86_STORE_U32(dst_{base}, X86_LOAD_U32(src_{base}));
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
            X86_STORE_U32(ctx.edi.u32, X86_LOAD_U32(ctx.esi.u32));
            ctx.esi.u32 += ({backward}) ? -4 : 4;
            ctx.edi.u32 += ({backward}) ? -4 : 4;
        "
        );
    }
}
