use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    cpp!(
        ctx.out,
        "
        {fpu}.st0() = -{fpu}.st0();
        {fpu}.sw &= ~X87FPU::C1;
    "
    );
}
