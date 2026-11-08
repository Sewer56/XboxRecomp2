//! Code shared by `and`, `or` and `xor`.

use super::super::prelude::*;

pub(super) fn handle_bitwise(
    ctx: &mut Emitter<'_>,
    op: &'static str,
    helper_prefix: &'static str,
    flag_updater: &'static str,
) {
    let flags = ctx.flag_template(false);
    let memory_flags = ctx.flag_template(true);
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return ctx.missing_form();
    }

    let dst = &operands[0];
    let src = &operands[1];
    let size_suffix = operand_size_name(dst.size);

    if memory(dst).is_some() {
        let src_str = if let Some(imm) = immediate(src) {
            format!("{:#x}", mask_imm(size_suffix, imm.value))
        } else {
            ctx.format_operand(src, size_suffix)
        };

        let size = dst.size;
        let dst_addr = ctx.ea(dst);
        cpp!(
            ctx.out,
            "X86_{helper_prefix}{size}_MEM{memory_flags}({dst_addr}, {src_str}, ctx, base);"
        );
    } else {
        if segment_equals(src, Register::DS) {
            //read
            let offset = memory(src).map_or(0, |mem| mem.disp.displacement as u32);

            let src_str = match ctx.route(Register::DS, offset, src.size, false) {
                MemoryRoute::Expr(expr) => expr.replace("{reg}", &array_subscript(src, src.size)),
                _ => ctx.format_operand(src, size_suffix),
            };

            let dst_str = ctx.format_operand(dst, size_suffix);
            cpp!(ctx.out, "{dst_str} {op} {src_str};");
        } else if let Some(imm) = immediate(src) {
            let dst_str = ctx.format_operand(dst, size_suffix);
            let value = mask_imm(size_suffix, imm.value);
            cpp!(ctx.out, "{dst_str} {op} {value:#x};");
        } else {
            let dst_str = ctx.format_operand(dst, size_suffix);
            let src_str = ctx.format_operand(src, size_suffix);
            cpp!(ctx.out, "{dst_str} {op} {src_str};");
        }

        let dst_str = ctx.format_operand(dst, size_suffix);
        cpp!(ctx.out, "ctx.eflags.{flag_updater}{flags}({dst_str});");
    }
}

#[inline]
fn mask_imm(size_suffix: &str, imm: u64) -> u64 {
    match size_suffix {
        "U8" => imm & 0xFF,
        "U16" => imm & 0xFFFF,
        _ => imm & 0xFFFF_FFFF,
    }
}
