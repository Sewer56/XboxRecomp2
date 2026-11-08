use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];

    //xbox kernel stuff
    if segment_equals(dst, Register::FS) {
        let size = ctx.insn.instruction().address_width;
        let offset = memory(dst).map_or(0, |mem| mem.disp.displacement as u32);
        match ctx.route(Register::FS, offset, u16::from(size), false) {
            MemoryRoute::Statement(template) => ctx.emit_platform_statement(&template, ""),
            _ => ctx.missing_form(),
        }
        return;
    }

    let is_16 = ctx.insn.instruction().operand_width == 16;
    let push = if is_16 { "X86_PUSH16" } else { "X86_PUSH" };
    if register(dst).is_some() {
        //PUSH reg
        let value = get_x86_reg(register(dst).unwrap());
        cpp!(ctx.out, "{push}(ctx, base, {value});");
    } else if immediate(dst).is_some() {
        //PUSH imm
        let imm_value = immediate(dst).unwrap().value as u32;
        let imm_value = if is_16 { imm_value & 0xFFFF } else { imm_value };
        cpp!(ctx.out, "{push}(ctx, base, 0x{imm_value:X});");
    } else if memory(dst).is_some() {
        //PUSH [mem]
        let size_name = operand_size_name(dst.size);
        let dst_ea = ctx.ea(dst);
        cpp!(
            ctx.out,
            "{push}(ctx, base, X86_LOAD_{size_name}({dst_ea}));"
        );
    } else {
        ctx.missing_form();
    }
}
