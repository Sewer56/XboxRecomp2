use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let Some((lhs, rhs)) = super::super::compare::operands(ctx) else {
        return;
    };
    let size_suffix = operand_size_name(ctx.insn.operands()[0].size);
    let flags = ctx.flag_template(false);
    cpp!(
        ctx.out,
        "ctx.eflags.update_compare_flags_{size_suffix}{flags}({lhs}, {rhs});"
    );
}
