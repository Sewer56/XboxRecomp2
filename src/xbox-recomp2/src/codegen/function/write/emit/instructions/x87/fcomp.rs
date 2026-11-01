use super::super::prelude::*;
use super::helpers::handle_fcom;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_fcom(ctx, 1);
}
