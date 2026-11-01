use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    //(FSTSW) Store FPU Status Word
    let operands = ctx.insn.operands();

    if register(&operands[0]) == Some(Register::AX) {
        cpp!(ctx.out, "ctx.eax.u16 = {fpu}.sw;");
    } else if memory(&operands[0]).is_some() {
        let addr = ea(&operands[0]);
        cpp!(ctx.out, "X86_STORE_U16({addr}, {fpu}.sw);");
    } else {
        ctx.missing_form();
    }
}
