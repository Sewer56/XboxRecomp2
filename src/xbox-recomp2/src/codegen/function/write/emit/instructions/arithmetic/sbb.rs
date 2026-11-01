use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let size_suffix = operand_size_name(dst.size);
    let size = dst.size;

    let src_str = if register(src).is_some() {
        get_x86_reg(register(src).unwrap())
    } else if immediate(src).is_some() {
        immediate(src).unwrap().value.to_string()
    } else {
        format!("X86_LOAD_{size_suffix}({})", ea(src))
    };

    if memory(dst).is_some() {
        let dst_ea = ea(dst);
        cpp!(
            ctx.out,
            "X86_SBB{size}_MEM{memory_flags}({dst_ea}, {src_str}, ctx, base);"
        );
    } else if let Some(dst_reg) = register(dst) {
        let helper = format!("X86_SBB{size}");
        super::write_register_op(ctx, dst_reg, &helper, &format!("{src_str}, "));
    } else {
        ctx.missing_form();
    }
}
