use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    match (memory(&operands[0]), register(&operands[1])) {
        (Some(_), Some(src)) if is_mm0_to_7(src) => {
            let src_reg = ctx.mmx_reg(mmx_index(src));
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_NTQ({addr}, {src_reg});");
        }
        _ => ctx.missing_form(),
    }
}
