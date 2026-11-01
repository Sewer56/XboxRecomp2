use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let flags = ctx.flag_template(false);
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let size = dst.size;

    // Count is masked to 5 bits; 0 is a no-op.
    let count_str = if let Some(imm) = immediate(&operands[1]) {
        let count = imm.value & 31;
        if count == 0 {
            return;
        }
        count.to_string()
    } else if register(&operands[1]) == Some(Register::CL) {
        "ctx.ecx.u8_lo".to_string()
    } else {
        return ctx.missing_form();
    };

    match dst.size {
        8 | 16 | 32 if register(dst).is_some() => {
            let dst_str = get_x86_reg(register(dst).unwrap());
            cpp!(
                ctx.out,
                "X86_SAR{size}{flags}({dst_str}, {count_str}, ctx);"
            );
        }
        8 | 16 | 32 if memory(dst).is_some() => {
            let dst_ea = ea(dst);
            cpp!(
                ctx.out,
                "X86_SAR{size}_MEM{memory_flags}({dst_ea}, {count_str}, ctx, base);"
            );
        }
        _ => ctx.missing_form(),
    }
}
