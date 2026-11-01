use super::super::prelude::*;
use super::div::write_divide;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    write_divide(ctx, "X86_IDIV");
}
