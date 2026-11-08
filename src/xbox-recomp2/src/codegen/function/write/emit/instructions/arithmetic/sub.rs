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

    // Resolve source
    let src_str = if immediate(src).is_some() {
        (immediate(src).unwrap().value as u32 as i32).to_string()
    } else if memory(src).is_some() {
        format!("X86_LOAD_{size_suffix}({})", ctx.ea(src))
    } else if let Some(reg) = register(src) {
        get_x86_reg(reg)
    } else {
        return ctx.missing_form();
    };

    match dst.size {
        8 | 16 | 32 if register(dst).is_some() => {
            let helper = format!("X86_SUB{size}");
            super::write_register_op(
                ctx,
                register(dst).unwrap(),
                &helper,
                &format!("{src_str}, "),
            );
        }
        8 | 16 | 32 if memory(dst).is_some() => {
            let dst_ea = ctx.ea(dst);
            cpp!(
                ctx.out,
                "X86_SUB{size}_MEM{memory_flags}({dst_ea}, {src_str}, ctx, base);"
            );
        }
        _ => ctx.missing_form(),
    }
}
