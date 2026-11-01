use super::super::prelude::*;
use super::helpers::float_operations;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    float_operations(ctx, "-", ctx.insn.operands()[0].size);
}
