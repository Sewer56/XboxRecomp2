use super::super::prelude::*;

const DEBUG_SERVICE: u8 = 0x2D;

// With no debugger to talk to, the debug service has nothing to do.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let vector = immediate(&ctx.insn.operands()[0]).map(|imm| imm.value as u8);
    if vector != Some(DEBUG_SERVICE) {
        ctx.missing_form();
    }
}
