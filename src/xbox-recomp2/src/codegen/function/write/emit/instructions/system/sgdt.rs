use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let dst = ea(&operands[0]);
    cpp!(
        ctx.out,
        "
        X86_STORE_U16({dst}, ctx.gdtr.limit);
        X86_STORE_U32({dst}+2, ctx.gdtr.base);
    "
    );
}
