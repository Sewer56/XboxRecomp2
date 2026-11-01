use super::super::prelude::*;
use super::helpers::{emit_precision_flag, emit_st0_exception_flags, stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let count = usize::from(ctx.insn.instruction().operand_count);
    let checked = stack_template_argument(ctx);

    if count == 0 {
        cpp!(ctx.out, "{fpu}.st1() *= {fpu}.st0();");
        emit_precision_flag(ctx);
        emit_st0_exception_flags(ctx);
        cpp!(ctx.out, "{fpu}.pop{checked}();");
    } else if count >= 2 {
        let dest = &operands[0];
        let src = &operands[1];

        if let (Some(dest_reg), Some(src_reg)) = (register(dest), register(src)) {
            let dest_idx = st_index(dest_reg);
            let src_idx = st_index(src_reg);
            cpp!(
                ctx.out,
                "
                {fpu}.st{dest_idx}() *= {fpu}.st{src_idx}();
                {fpu}.pop{checked}();
            "
            );
            emit_st0_exception_flags(ctx);
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}
