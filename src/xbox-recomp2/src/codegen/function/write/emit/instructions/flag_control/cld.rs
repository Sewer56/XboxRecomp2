use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(
        ctx.out,
        "ctx.eflags.set_df(false);  // Set forward direction"
    );
}
