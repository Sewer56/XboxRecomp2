use super::super::prelude::*;
use super::write_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let size = dst.size;

    let is_system = |op: &DecodedOperand| {
        register(op)
            .is_some_and(|reg| matches!(reg.class(), RegisterClass::CONTROL | RegisterClass::DEBUG))
    };
    if is_system(dst) || is_system(src) {
        return ctx.missing_form();
    }

    let dst_str = get_x86_reg(operand_reg_value(dst)); // Always a register for MOV

    //hacky way to fix indirect call using kernel address through register
    if register(dst).is_some() && memory(src).is_some() {
        let offset = memory(src).unwrap().disp.displacement as u32;
        if ctx.imports.resolve_thunk(offset.into()).is_some() {
            ctx.function_state.has_register_import = true;
            ctx.function_state.register_import_thunk = offset;
        }
    }

    //FS is Xbox kernel stuff
    //handle FS register (kernel and xbox memory stuff, e.g "FS:[0x20]")
    if memory(src).is_some() && segment_equals(src, Register::FS) {
        // MOV reg, FS:[offset]
        let offset = memory(src).unwrap().disp.displacement as u32;
        match ctx.route(Register::FS, offset, size, false) {
            MemoryRoute::Statement(template) => ctx.emit_platform_statement(&template, &dst_str),
            _ => ctx.missing_form(),
        }
        return;
    }
    if memory(dst).is_some() && segment_equals(dst, Register::FS) {
        // MOV FS:[offset], reg
        let offset = memory(dst).unwrap().disp.displacement as u32;
        let src_str = format_operand(src, operand_size_name(size));
        match ctx.route(Register::FS, offset, size, true) {
            MemoryRoute::Statement(template) => ctx.emit_platform_statement(&template, &src_str),
            _ => ctx.missing_form(),
        }
        return;
    }

    //DS can be hardware Xbox
    //In that case, we identify MMIO access and redirect the addresses to a global array
    if memory(src).is_some() && segment_equals(src, Register::DS) {
        //read
        let offset = memory(src).unwrap().disp.displacement as u32;
        match ctx.route(Register::DS, offset, src.size, false) {
            MemoryRoute::Statement(template) => {
                let reg = get_x86_reg(operand_reg_value(dst));
                ctx.emit_platform_statement(&template, &reg);
                return;
            }
            MemoryRoute::Expr(array) => {
                let array = array.replace("{reg}", &array_subscript(src, src.size));
                cpp!(ctx.out, "{dst_str} = {array};  //Read Signature array");
            }
            MemoryRoute::Plain => {}
        }
    } else if memory(dst).is_some() && segment_equals(dst, Register::DS) {
        //write
        let offset = memory(dst).unwrap().disp.displacement as u32;
        let value = format_operand(src, operand_size_name(size));
        match ctx.route(Register::DS, offset, dst.size, true) {
            MemoryRoute::Statement(template) => {
                ctx.emit_platform_statement(&template, &value);
                return;
            }
            MemoryRoute::Expr(array) => {
                let array = array.replace("{reg}", &array_subscript(dst, dst.size));
                cpp!(ctx.out, "{array} = {value};  //Write in Signature array");
            }
            MemoryRoute::Plain => {}
        }
    }

    let src_str = format_operand(src, operand_size_name(size));

    if !src_str.is_empty() {
        // Special case for memory destinations
        if memory(dst).is_some() {
            let size_suffix = operand_size_name(size);
            let dst_ea = ea(dst);
            cpp!(ctx.out, "X86_STORE_{size_suffix}({dst_ea}, {src_str});");
        } else if let Some(dst_reg) = register(dst) {
            write_register(ctx, dst_reg, &src_str);
        } else {
            cpp!(ctx.out, "{dst_str} = {src_str};");
        }
    } else {
        ctx.missing_form();
    }
}
