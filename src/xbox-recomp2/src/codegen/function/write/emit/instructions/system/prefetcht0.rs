use super::super::prelude::*;

// Only a cache hint, with no visible effect. A host prefetch measured no faster.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        ctx.missing_form();
    }
}
