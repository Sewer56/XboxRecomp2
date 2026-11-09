use super::super::prelude::*;
use crate::codegen::function::model::DirectionFlag;

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
        super::try_memmove(ctx, "uint8_t");
        cpp!(
            ctx.out,
            "
            {{
            const {step} step_{base} = ({backward}) ? -1 : 1;
            uint32_t count_{base} = ctx.ecx.u32;
            {address} src_{base} = ctx.esi.u32, dst_{base} = ctx.edi.u32;
            for (; count_{base} > 0; count_{base}--, src_{base} += step_{base}, dst_{base} += step_{base}) {{
                X86_STORE_U8(dst_{base}, X86_LOAD_U8(src_{base}));
            }}
            ctx.esi.u32 = static_cast<uint32_t>(src_{base});
            ctx.edi.u32 = static_cast<uint32_t>(dst_{base});
            ctx.ecx.u32 = 0;
            }}
        "
        );
    } else if ctx.direction == DirectionFlag::Unknown {
        cpp!(
            ctx.out,
            "
            X86_STORE_U8(ctx.edi.u32, X86_LOAD_U8(ctx.esi.u32));
            ctx.eflags.update_string_pointer(ctx.esi.u32);
            ctx.eflags.update_string_pointer(ctx.edi.u32);
        "
        );
    } else {
        cpp!(
            ctx.out,
            "
            X86_STORE_U8(ctx.edi.u32, X86_LOAD_U8(ctx.esi.u32));
            ctx.esi.u32 += ({backward}) ? -1 : 1;
            ctx.edi.u32 += ({backward}) ? -1 : 1;
        "
        );
    }
}
