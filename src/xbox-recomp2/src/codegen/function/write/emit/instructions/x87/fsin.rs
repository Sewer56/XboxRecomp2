use super::super::prelude::*;
use super::helpers::{emit_precision_flag, precision_flag_line};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    // Intel's manual leaves C0 and C3 undefined after FSIN, so we keep them.
    if ctx.config.speed_hacks.runs_fast_trig() {
        // libm takes any angle, so no range check.
        cpp!(
            ctx.out,
            "
            {fpu}.sw &= ~(X87FPU::C1 | X87FPU::C2);
            val_double_0 = {fpu}.st0();
            {fpu}.st0() = ((x87_float)sin((double)val_double_0));"
        );
        return emit_precision_flag(ctx);
    }

    let precision = precision_flag_line(ctx);
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~(X87FPU::C1 | X87FPU::C2);
        val_double_0 = {fpu}.st0();
        if (X87_TRIG_OUT_OF_RANGE(val_double_0)) {{
            {fpu}.sw |= X87FPU::C2; // Out of range: ST(0) unchanged
        }} else {{
            {fpu}.st0() = (std::sin(val_double_0));{precision}
        }}
    "
    );
}
