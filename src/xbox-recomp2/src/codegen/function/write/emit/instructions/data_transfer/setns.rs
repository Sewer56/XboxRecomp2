use super::super::prelude::*;
use super::write_setcc;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    write_setcc(ctx, "((ctx.eflags.common & X86EFlags::SF) == 0)");
}
