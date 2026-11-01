use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(ctx.out, "X86_PUSH(ctx, base, ctx.eflags.packed());");
}
