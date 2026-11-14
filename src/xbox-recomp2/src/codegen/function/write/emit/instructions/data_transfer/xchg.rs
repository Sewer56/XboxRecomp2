use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let op1 = &operands[0];
    let op2 = &operands[1];
    let atomic_suffix = operand_size_name(op1.size);

    // Speed hack `plain_lock_release`: unlock with a plain store.
    if ctx.releases_lock {
        let (mem, reg) = match (register(op1), register(op2)) {
            (Some(reg), None) => (op2, reg),
            (None, Some(reg)) => (op1, reg),
            _ => return ctx.missing_form(),
        };
        let reg_str = get_x86_reg(reg);
        let ea = ctx.ea(mem);
        cpp!(
            ctx.out,
            "{reg_str} = X86_RELEASE_LOCK_{atomic_suffix}({ea});"
        );
        return;
    }

    if register(op1).is_some() && register(op2).is_some() {
        let op1_str = get_x86_reg(register(op1).unwrap());
        let op2_str = get_x86_reg(register(op2).unwrap());
        cpp!(ctx.out, "X86_XCHG_REG({op1_str}, {op2_str});");
    } else if register(op1).is_some() && memory(op2).is_some() {
        let op1_str = get_x86_reg(register(op1).unwrap());
        let op2_ea = ctx.ea(op2);
        cpp!(
            ctx.out,
            "{op1_str} = X86_ATOMIC_XCHG_{atomic_suffix}({op2_ea}, {op1_str});"
        );
    } else if memory(op1).is_some() && register(op2).is_some() {
        let op2_str = get_x86_reg(register(op2).unwrap());
        let op1_ea = ctx.ea(op1);
        cpp!(
            ctx.out,
            "{op2_str} = X86_ATOMIC_XCHG_{atomic_suffix}({op1_ea}, {op2_str});"
        );
    } else {
        ctx.missing_form();
    }
}
