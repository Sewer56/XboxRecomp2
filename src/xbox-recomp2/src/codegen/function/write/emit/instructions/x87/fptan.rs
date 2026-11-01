use super::super::prelude::*;
use super::helpers::{
    emit_precision_flag, emit_st0_exception_flags, precision_flag_line, stack_template_argument,
};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let checked = stack_template_argument(ctx);

    if ctx.config.speed_hacks.runs_fast_trig() {
        // libm takes any angle, so no range check.
        cpp!(
            ctx.out,
            "
            {fpu}.sw &= ~X87FPU::C2;
            {fpu}.st0() = (x87_float)std::tan((double){fpu}.st0());"
        );
        emit_precision_flag(ctx);
        cpp!(
            ctx.out,
            "
            // A NaN result pushes NaN, not 1.0
            {fpu}.push{checked}({fpu}.st0() == {fpu}.st0() ? X87FPU::ONE : {fpu}.st0());"
        );
        return emit_st0_exception_flags(ctx);
    }

    let precision = precision_flag_line(ctx);
    let exceptions = if ctx.config.speed_hacks.runs_fast_x87_status() {
        String::new()
    } else {
        format!("\n    {fpu}.set_exception_flags({fpu}.st0());")
    };
    cpp!(
        ctx.out,
        "
        {fpu}.sw &= ~X87FPU::C2;
        if (X87_TRIG_OUT_OF_RANGE({fpu}.st0())) {{
            {fpu}.sw |= X87FPU::C2; // Out of range: no push
        }} else {{
            {fpu}.st0() = std::tan({fpu}.st0());{precision}
            // A NaN result pushes NaN, not 1.0
            {fpu}.push{checked}({fpu}.st0() == {fpu}.st0() ? X87FPU::ONE : {fpu}.st0());{exceptions}
        }}
    "
    );
}
