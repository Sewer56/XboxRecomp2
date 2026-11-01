use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let port = if let Some(imm) = immediate(&operands[0]) {
        format!("0x{:X}u", imm.value as u8)
    } else if register(&operands[0]) == Some(Register::DX) {
        get_x86_reg(Register::DX)
    } else {
        return ctx.missing_form();
    };
    let Some(src) = register(&operands[1]) else {
        return ctx.missing_form();
    };
    let width = match src {
        Register::AL => 8,
        Register::AX => 16,
        Register::EAX => 32,
        _ => return ctx.missing_form(),
    };
    let value = get_x86_reg(src);
    cpp!(ctx.out, "X86_OUT{width}(ctx, {port}, {value});");
}
