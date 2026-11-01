use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(
        ctx.out,
        "
        X86_EMMS();
        {fpu}.tw = 0xFFFF;
        {fpu}.top = 0;
    "
    );
}
