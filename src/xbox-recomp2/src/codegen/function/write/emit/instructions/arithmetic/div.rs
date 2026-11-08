use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    write_divide(ctx, "X86_DIV", "");
}

pub(super) fn write_divide(ctx: &mut Emitter<'_>, helper: &str, suffix: &str) {
    if ctx.insn.instruction().operand_count == 0 {
        return ctx.missing_form();
    }
    let op = &ctx.insn.operands()[0];
    let size = op.size;
    if !matches!(size, 8 | 16 | 32) {
        return ctx.missing_form();
    }

    let divisor = if let Some(reg) = register(op) {
        get_x86_reg(reg)
    } else if memory(op).is_some()
        && !segment_equals(op, Register::FS)
        && !segment_equals(op, Register::GS)
        && ctx.insn.instruction().address_width == 32
    {
        format!("X86_LOAD_{}({})", operand_size_name(size), ea(op))
    } else {
        return ctx.missing_form();
    };

    cpp!(
        ctx.out,
        "{helper}{size}{suffix}(static_cast<uint32_t>({divisor}), ctx);"
    );
}
