use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    // Operand 0: memory source
    let src = ea(&operands[0]);
    cpp!(
        ctx.out,
        "
        ctx.mxcsr = X86_LOAD_U32({src});
        _mm_setcsr(ctx.mxcsr | 0x1F80);
    "
    );
}
