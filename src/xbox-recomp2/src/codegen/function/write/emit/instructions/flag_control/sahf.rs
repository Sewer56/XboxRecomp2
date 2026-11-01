use super::super::prelude::*;

/// Writes SF, ZF, AF, PF and CF from AH and keeps OF.
///
/// Above its low byte, `common` only holds OF, so we keep those bits whole
/// and the compiler can store a single byte.
pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    cpp!(
        ctx.out,
        "ctx.eflags.common = (ctx.eflags.common & ~0xFFu) | (ctx.eax.u8_hi & (X86EFlags::SF | X86EFlags::ZF | X86EFlags::AF | X86EFlags::PF | X86EFlags::CF));"
    );
}
