use super::super::data_transfer::register_assignment;
use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst), Some(port)) = (register(&operands[0]), port(&operands[1])) else {
        return ctx.missing_form();
    };
    let width = match dst {
        Register::AL => 8,
        Register::AX => 16,
        Register::EAX => 32,
        _ => return ctx.missing_form(),
    };

    let assignment = register_assignment(dst, &format!("X86_IN{width}(ctx, {port})"));
    cpp!(ctx.out, "{assignment};");
}

fn port(operand: &DecodedOperand) -> Option<String> {
    if let Some(imm) = immediate(operand) {
        return Some(format!("0x{:X}u", imm.value as u8));
    }
    (register(operand) == Some(Register::DX)).then(|| get_x86_reg(Register::DX))
}
