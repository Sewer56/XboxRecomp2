use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 1 {
        return ctx.missing_form();
    }

    let dest = &operands[0];
    let size = dest.size;

    match dest.size {
        8 | 16 | 32 if register(dest).is_some() => {
            let helper = format!("X86_DEC{size}");
            super::write_register_op(ctx, register(dest).unwrap(), &helper, "");
        }
        8 | 16 | 32 if memory(dest).is_some() => {
            let dest_ea = ea(dest);
            cpp!(
                ctx.out,
                "X86_DEC{size}_MEM{memory_flags}({dest_ea}, ctx, base);"
            );
        }
        _ => ctx.missing_form(),
    }
}
