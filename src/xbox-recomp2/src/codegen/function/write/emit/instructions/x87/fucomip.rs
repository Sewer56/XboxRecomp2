use super::super::prelude::*;
use super::helpers::handle_fcomi;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_fcomi(ctx, false);
}
