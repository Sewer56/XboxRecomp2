use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    // The Xbox has no LDT.
    let operands = ctx.insn.operands();
    if let Some(dst) = register(&operands[0]) {
        let dst = get_x86_reg(dst);
        cpp!(ctx.out, "{dst} = 0;");
    } else if memory(&operands[0]).is_some() {
        let addr = ctx.ea(&operands[0]);
        cpp!(ctx.out, "X86_STORE_U16({addr}, 0);");
    }
}
