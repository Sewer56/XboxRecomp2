use super::super::prelude::*;
use super::helpers::handle_fdivr;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_fdivr(ctx);
}
