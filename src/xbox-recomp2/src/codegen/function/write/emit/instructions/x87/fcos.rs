use super::super::prelude::*;
use super::helpers::{emit_precision_flag, precision_flag_line, trig_status_lines};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let (clear, out_of_range) = trig_status_lines(ctx, ConditionCodes::C2);
    if ctx.config.speed_hacks.runs_fast_trig() {
        // libm takes any angle, so no range check.
        cpp!(
            ctx.out,
            "
            val_double_0 = {fpu}.st0();
            {clear}{fpu}.st0() = (x87_float)cos((double)val_double_0);"
        );
        return emit_precision_flag(ctx);
    }

    let precision = precision_flag_line(ctx);
    cpp!(
        ctx.out,
        "
        val_double_0 = {fpu}.st0();
        {clear}if (X87_TRIG_OUT_OF_RANGE(val_double_0)) {{
            {out_of_range}// Out of range: ST(0) unchanged
        }} else {{
            {fpu}.st0() = std::cos(val_double_0);{precision}
        }}
    "
    );
}
