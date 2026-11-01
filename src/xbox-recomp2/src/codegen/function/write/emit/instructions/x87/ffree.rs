use super::super::prelude::*;
use super::helpers::get_st_index;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let index = get_st_index(operand_reg_value(&ctx.insn.operands()[0]));

    cpp!(ctx.out, "{fpu}.ffree({index});");
}
