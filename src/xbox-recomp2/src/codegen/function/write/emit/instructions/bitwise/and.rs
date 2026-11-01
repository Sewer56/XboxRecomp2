use super::super::prelude::*;
use super::helpers::handle_bitwise;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_bitwise(ctx, "&=", "AND", "update_flags_logic");
}
