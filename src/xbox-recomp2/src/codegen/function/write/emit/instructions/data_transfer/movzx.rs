use super::super::prelude::*;
use super::write_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];

    if memory(src).is_some() && segment_equals(src, Register::FS) {
        let offset = memory(src).unwrap().disp.displacement as u32;
        let dst_reg = register(dst).unwrap();
        let dst_type = operand_type_name(dst.size);
        match ctx.route(Register::FS, offset, src.size, false) {
            MemoryRoute::Expr(load) => {
                write_register(ctx, dst_reg, &format!("({dst_type}){load}"));
            }
            _ => ctx.missing_form(),
        }
        return;
    }

    if register(src).is_some() {
        let dst_reg = register(dst).unwrap();
        let src_str = get_x86_reg(register(src).unwrap());

        if dst.size == 32 {
            if src.size == 8 {
                let src_reg = register(src).unwrap();
                if let Some(base_reg) = high_byte_parent(src_reg) {
                    // High byte register (AH, CH, DH, BH)
                    let base_str = get_x86_reg(base_reg);
                    write_register(ctx, dst_reg, &format!("(uint8_t)({base_str} >> 8)"));
                } else {
                    // Low byte register (AL, CL, DL, BL)
                    write_register(ctx, dst_reg, &format!("(uint8_t){src_str}"));
                }
            } else if src.size == 16 {
                write_register(ctx, dst_reg, &format!("{src_str} & 0xFFFF"));
            } else {
                ctx.missing_form();
            }
        } else if dst.size == 16 && src.size == 16 {
            write_register(ctx, dst_reg, &src_str);
        } else if dst.size == 16 && src.size == 8 {
            // 16-bit destination
            // 8-bit source
            let src_reg = register(src).unwrap();
            if let Some(base_reg) = high_byte_parent(src_reg) {
                // High byte register (AH, CH, DH, BH)
                let base_str = get_x86_reg(base_reg);
                write_register(ctx, dst_reg, &format!("(uint8_t)({base_str} >> 8)"));
            } else {
                // Low byte register (AL, CL, DL, BL)
                write_register(ctx, dst_reg, &format!("(uint8_t){src_str}"));
            }
        } else {
            ctx.missing_form();
        }
    } else if memory(src).is_some() {
        // Determine load size from SOURCE operand
        let load_op = match src.size {
            8 => "X86_LOAD_U8",
            16 => "X86_LOAD_U16",
            _ => return ctx.missing_form(),
        };

        let dst_reg = register(dst).unwrap();
        let dst_type = operand_type_name(dst.size);
        let memory_addr = ctx.ea(src);
        write_register(
            ctx,
            dst_reg,
            &format!("({dst_type}){load_op}({memory_addr})"),
        );
    } else {
        ctx.missing_form();
    }
}

fn high_byte_parent(reg: Register) -> Option<Register> {
    match reg {
        Register::AH => Some(Register::EAX),
        Register::CH => Some(Register::ECX),
        Register::DH => Some(Register::EDX),
        Register::BH => Some(Register::EBX),
        _ => None,
    }
}
