use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    match (xmm_register(&operands[0]), xmm_register(&operands[1])) {
        (Some(dst_idx), Some(src_idx)) => cpp!(ctx.out, "X86_MOVHLPS({dst_idx}, {src_idx});"),
        _ => ctx.missing_form(),
    }
}
