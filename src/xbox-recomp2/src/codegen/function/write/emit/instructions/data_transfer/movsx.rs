use super::super::prelude::*;
use super::write_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let src_size_suffix = operand_size_name(src.size);

    //handle FS register (kernel and xbox memory stuff, e.g "FS:[0x20]")
    if memory(src).is_some() && segment_equals(src, Register::FS) {
        let offset = memory(src).unwrap().disp.displacement as u32;
        let dst_str = get_x86_reg(register(dst).unwrap());

        match ctx.route(Register::FS, offset, src.size, false) {
            MemoryRoute::Statement(template) => ctx.emit_platform_statement(&template, &dst_str),
            _ => ctx.missing_form(),
        }
        return;
    }

    if register(src).is_some() {
        //Handle sign extension from smaller to larger register
        let dst_reg = register(dst).unwrap();
        let src_str = get_x86_reg(register(src).unwrap());

        if src.size == 8 {
            let dst_type = if dst.size == 16 { "int16_t" } else { "int32_t" }; // destination type
            write_register(
                ctx,
                dst_reg,
                &format!("static_cast<{dst_type}>(static_cast<int8_t>({src_str}))"),
            );
        } else if src.size == 16 {
            write_register(
                ctx,
                dst_reg,
                &format!("static_cast<int32_t>(static_cast<int16_t>({src_str}))"),
            );
        } else {
            ctx.missing_form();
        }
    } else if memory(src).is_some() {
        //Handle memory source with sign extension
        let dst_reg = register(dst).unwrap();
        let src_ea = ctx.ea(src);

        if src.size == 8 {
            let dst_type = if dst.size == 16 { "int16_t" } else { "int32_t" }; //destination type
            let value = format!(
                "static_cast<{dst_type}>(static_cast<int8_t>(X86_LOAD_{src_size_suffix}({src_ea})))"
            );
            write_register(ctx, dst_reg, &value);
        } else if src.size == 16 {
            let value = format!(
                "static_cast<int32_t>(static_cast<int16_t>(X86_LOAD_{src_size_suffix}({src_ea})))"
            );
            write_register(ctx, dst_reg, &value);
        } else {
            ctx.missing_form();
        }
    } else {
        ctx.missing_form();
    }
}
