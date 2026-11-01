use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();

    if let Some(reg) = register(&operands[0]) {
        let index = st_index(reg);
        cpp!(ctx.out, "{fpu}.st{index}() = {fpu}.st0();");
    } else if memory(&operands[0]).is_some() {
        if operands[0].size == 32 {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_F32({addr}, {fpu}.st0());");
        } else if operands[0].size == 64 {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_F64({addr}, {fpu}.st0());");
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}
