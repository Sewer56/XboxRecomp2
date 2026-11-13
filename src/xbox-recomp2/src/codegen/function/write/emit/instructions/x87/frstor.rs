use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();

    if memory(&operands[0]).is_some() {
        let mem_addr = ctx.ea(&operands[0]);

        if operands[0].size == 864 {
            //32 bits
            cpp!(ctx.out, "{fpu}.load({mem_addr}, base, true);");
        } else {
            cpp!(ctx.out, "{fpu}.load({mem_addr}, base, false);");
        }
    } else {
        ctx.missing_form();
    }
}
