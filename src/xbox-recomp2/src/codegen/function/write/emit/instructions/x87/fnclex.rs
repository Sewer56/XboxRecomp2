use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(
        ctx.out,
        "{fpu}.sw &= ~0x80FF; //Clear the exception flags, SF, ES and B (bits 0-7 and 15)"
    );
}
