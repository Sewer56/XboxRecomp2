use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if memory(&operands[0]).is_none() {
        return ctx.missing_form();
    }

    // `x87_out` doesn't copy `cw` back, so a local copy writes both.
    let addr = ctx.ea(&operands[0]);
    if ctx.function_state.local_fpu {
        cpp!(ctx.out, "fpu.cw = ctx.fpu.cw = X86_LOAD_U16({addr});");
    } else {
        cpp!(ctx.out, "ctx.fpu.cw = X86_LOAD_U16({addr});");
    }
}
