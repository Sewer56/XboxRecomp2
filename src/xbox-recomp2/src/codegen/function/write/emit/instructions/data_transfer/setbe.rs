use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        return;
    }

    let dst = &operands[0];
    let cond = "( (ctx.eflags.common & (X86EFlags::CF | X86EFlags::ZF)) != 0)";

    if register(dst).is_some() {
        let dst_str = get_x86_reg(register(dst).unwrap());
        cpp!(ctx.out, "{dst_str} = {cond};");
    } else if memory(dst).is_some() {
        let dst_ea = ctx.ea(dst);
        cpp!(ctx.out, "X86_STORE_U8({dst_ea}, {cond});");
    }
}
