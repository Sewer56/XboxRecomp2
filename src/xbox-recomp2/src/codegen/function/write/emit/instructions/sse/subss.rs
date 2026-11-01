use super::super::prelude::*;
use super::helpers::handle_sse_scalar_instruction;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_sse_scalar_instruction(ctx, "_mm_sub_ss");
}
