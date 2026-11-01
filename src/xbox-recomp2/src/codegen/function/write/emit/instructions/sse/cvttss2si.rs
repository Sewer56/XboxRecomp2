use super::super::prelude::*;
use super::helpers::handle_cvtss2si;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    handle_cvtss2si(ctx);
}
