use super::super::prelude::*;
use super::write_cmov;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // CMOVGE: Move if SF == OF
    write_cmov(
        ctx,
        "((ctx.eflags.common & X86EFlags::SF) != 0) == ((ctx.eflags.common & X86EFlags::OF) != 0)",
    );
}
