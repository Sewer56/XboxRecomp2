use super::super::prelude::*;
use super::div::write_divide;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    write_divide(ctx, "X86_IDIV", "");
}

pub(in crate::codegen::function::write) fn emit_after_cdq(ctx: &mut Emitter<'_>) {
    write_divide(ctx, "X86_IDIV", "_CDQ");
}
