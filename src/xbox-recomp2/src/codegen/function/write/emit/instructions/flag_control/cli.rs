use super::super::prelude::*;

// Only the flag changes: other guest threads still run, unlike on the Xbox's single core.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(ctx.out, "ctx.eflags.rare &= ~X86EFlags::IF;");
}
