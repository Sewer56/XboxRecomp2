use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    if memory(&operands[0]).is_some() {
        let addr = ea(&operands[0]);
        cpp!(ctx.out, "{fpu}.cw = X86_LOAD_U16({addr});");
    } else {
        ctx.missing_form();
    }
}
