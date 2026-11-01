use super::super::prelude::*;
use super::register_assignment;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let flags = ctx.flag_template(false);
    let operands = ctx.insn.operands();

    let operand_str = |op: &DecodedOperand| -> String {
        if register(op).is_some() {
            get_x86_reg(register(op).unwrap())
        } else if memory(op).is_some() {
            format!("X86_LOAD_{}({})", operand_size_name(op.size), ea(op))
        } else {
            format!("{}", immediate(op).unwrap().value as u32 as i32)
        }
    };

    let explicit_operands = operands
        .iter()
        .take(usize::from(ctx.insn.instruction().operand_count))
        .filter(|op| op.visibility == OperandVisibility::EXPLICIT)
        .count();

    match explicit_operands {
        1 => {
            //Form 1: EAX * r/m -> EDX:EAX
            let src = &operands[0];
            let rhs = operand_str(src);
            let line = match src.size {
                8 => format!("X86_IMUL_EAX_U8{flags}(static_cast<uint8_t>({rhs}), ctx);"),
                16 => format!("X86_IMUL_EAX_U16{flags}(static_cast<uint16_t>({rhs}), ctx);"),
                _ => format!("X86_IMUL_EAX_U32{flags}(static_cast<uint32_t>({rhs}), ctx);"),
            };
            ctx.out.line(&line);
        }
        2 => {
            // Form 2/3: dst = dst * r/m  OR  dst = dst * imm
            let rhs = operand_str(&operands[1]);
            write_imul(ctx, register(&operands[0]).unwrap(), None, &rhs);
        }
        3 => {
            // Form 4: dst = src * imm
            let rhs = operand_str(&operands[1]);
            let imm = operand_str(&operands[2]);
            write_imul(ctx, register(&operands[0]).unwrap(), Some(&rhs), &imm);
        }
        _ => ctx.missing_form(),
    }
}

/// Writes `dst = lhs * rhs` with IMUL flags, where no `lhs` means `dst` itself.
/// 8-bit and 16-bit results merge into the 32-bit register.
fn write_imul(ctx: &mut Emitter<'_>, dst: Register, lhs: Option<&str>, rhs: &str) {
    let flags = ctx.flag_template(false);
    let dst_str = get_x86_reg(dst);
    let (bits, helper) = match dst.class() {
        RegisterClass::GPR8 => (8, "X86_IMUL_U8_VAL"),
        RegisterClass::GPR16 => (16, "X86_IMUL_U16_VAL"),
        _ => (32, "X86_IMUL_U32"),
    };
    let lhs = lhs.map_or(dst_str.clone(), |lhs| {
        format!("static_cast<uint{bits}_t>({lhs})")
    });
    let rhs = format!("static_cast<uint{bits}_t>({rhs})");
    let statement = if bits == 32 {
        format!("{helper}{flags}({dst_str}, {lhs}, {rhs}, ctx)")
    } else {
        register_assignment(dst, &format!("{helper}{flags}({lhs}, {rhs}, ctx)"))
    };
    cpp!(ctx.out, "{statement};");
}
