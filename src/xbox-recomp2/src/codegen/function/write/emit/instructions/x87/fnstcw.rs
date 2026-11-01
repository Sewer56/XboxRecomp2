use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();

    if memory(&operands[0]).is_some() {
        let addr = ea(&operands[0]);
        cpp!(ctx.out, "X86_STORE_U16({addr}, {fpu}.cw);");
    } else {
        ctx.missing_form();
    }
}
