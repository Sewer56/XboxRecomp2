use super::super::prelude::*;
use super::helpers::{get_st_index, stack_template_argument};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();

    // The `fast_x87_ffree` speed hack
    if ctx.function_state.x87_sync.ffree_pops {
        let checked = stack_template_argument(ctx);
        cpp!(ctx.out, "{fpu}.pop{checked}();");
        return;
    }

    let index = get_st_index(operand_reg_value(&ctx.insn.operands()[0]));
    cpp!(ctx.out, "{fpu}.ffree({index});");
}
