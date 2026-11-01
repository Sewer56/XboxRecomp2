use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    super::helpers::compare_ss(ctx, true);
}
