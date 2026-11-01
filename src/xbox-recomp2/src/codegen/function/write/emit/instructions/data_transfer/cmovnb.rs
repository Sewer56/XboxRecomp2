use super::super::prelude::*;
use super::write_cmov;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    write_cmov(ctx, "(ctx.eflags.common & X86EFlags::CF) == 0");
}
