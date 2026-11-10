use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // It only sets C0-C3.
    if ctx.needed_status.is_empty() {
        return;
    }

    let fpu = ctx.fpu();
    cpp!(ctx.out, "X87_FXAM({fpu}, {fpu}.top);");
}
