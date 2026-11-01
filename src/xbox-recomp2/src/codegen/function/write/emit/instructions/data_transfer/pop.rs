use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];

    //xbox kernel stuff
    if segment_equals(dst, Register::FS) {
        let offset = memory(dst).map_or(0, |mem| mem.disp.displacement as u32);
        match ctx.route(Register::FS, offset, dst.size, true) {
            MemoryRoute::Statement(template) => ctx.emit_platform_statement(&template, ""),
            _ => ctx.missing_form(),
        }
        return;
    }

    let pop = if ctx.insn.instruction().operand_width == 16 {
        "X86_POP16"
    } else {
        "X86_POP"
    };
    if register(dst).is_some() {
        let dst = get_x86_reg(register(dst).unwrap());
        cpp!(ctx.out, "{dst} = {pop}(ctx, base);");
    } else if memory(dst).is_some() {
        // The address sees ESP after the pop (C++17 evaluation order).
        let size_suffix = operand_size_name(dst.size);
        let dst_ea = ea(dst);
        cpp!(
            ctx.out,
            "X86_STORE_{size_suffix}({dst_ea}, {pop}(ctx, base));"
        );
    } else {
        ctx.missing_form();
    }
}
