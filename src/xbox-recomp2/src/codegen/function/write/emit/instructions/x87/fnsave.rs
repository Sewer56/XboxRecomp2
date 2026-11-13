use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    //note, some instruction like "9B DD 71 08" will show as "fsave" on ida, but zydis detect these as fwait + fsave.
    let operands = ctx.insn.operands();

    if memory(&operands[0]).is_some() {
        let mem_addr = ctx.ea(&operands[0]);

        if operands[0].size == 864 {
            //32 bits
            cpp!(ctx.out, "{fpu}.save({mem_addr}, base, true);");
        } else {
            cpp!(ctx.out, "{fpu}.save({mem_addr}, base, false);");
        }

        cpp!(ctx.out, "{fpu}.init();");
    } else {
        ctx.missing_form();
    }
}
