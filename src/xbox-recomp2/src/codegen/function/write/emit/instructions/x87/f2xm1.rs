use super::super::prelude::*;
use super::helpers::emit_precision_flag;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    //F2XM1 - Compute 2^ST(0) - 1
    // Intel's manual leaves C0, C2 and C3 undefined after F2XM1, so we keep them.
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~X87FPU::C1;
        val_double_0 = {fpu}.st0();"
    );
    if ctx.config.speed_hacks.runs_fast_trig() {
        cpp!(
            ctx.out,
            "{fpu}.st0() = (x87_float)X87_F2XM1((double)val_double_0);"
        );
    } else {
        cpp!(
            ctx.out,
            "
            // expm1 avoids the cancellation of exp2(x) - 1 near 0
            {fpu}.st0() = std::expm1(val_double_0 * 0.6931471805599453094172321214581766L);"
        );
    }
    emit_precision_flag(ctx);
}
