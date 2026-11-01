use super::super::prelude::*;
use super::helpers::{emit_precision_flag, precision_flag_line, stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let checked = stack_template_argument(ctx);

    // Intel's manual leaves C0 and C3 undefined after FSINCOS, so we keep them.
    if ctx.config.speed_hacks.runs_fast_trig() {
        // libm takes any angle, so no range check.
        cpp!(
            ctx.out,
            "
            {fpu}.sw &= ~(X87FPU::C1 | X87FPU::C2);
            val_double_2 = {fpu}.st0();
            X87_FSINCOS{checked}({fpu}, (double)val_double_2);"
        );
        return emit_precision_flag(ctx);
    }

    let precision = precision_flag_line(ctx);
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~(X87FPU::C1 | X87FPU::C2);
        val_double_2 = {fpu}.st0();
        if (X87_TRIG_OUT_OF_RANGE(val_double_2)) {{
            {fpu}.sw |= X87FPU::C2; // Out of range: no push
        }} else {{
            X87_FSINCOS{checked}({fpu}, val_double_2);{precision}
        }}
    "
    );
}
