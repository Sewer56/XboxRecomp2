use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    match (memory(&operands[0]), xmm_register(&operands[1])) {
        (Some(_), Some(src_reg)) => {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_NTPS({addr}, ctx.xmm[{src_reg}].reg);");
        }
        _ => ctx.missing_form(),
    }
}
