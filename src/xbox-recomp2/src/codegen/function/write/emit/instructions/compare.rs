//! Reads the two operands of a CMP or TEST as C++ expressions.

use super::prelude::*;

/// Returns both operands of a CMP or TEST as C++ expressions.
///
/// # Examples
///
/// ```text
/// cmp eax, 0x20                      ->  ("ctx.eax.u32", "0x20u")
/// cmp cl, [edi]                      ->  ("ctx.ecx.u8_lo", "X86_LOAD_U8(ctx.edi.u32)")
/// test dword ptr ds:[0xFE800010], 1  ->  ("*(uint32_t*)&g_mmio[16]", "0x1u")  // Xbox
/// ```
pub(super) fn operands(ctx: &mut Emitter<'_>) -> Option<(String, String)> {
    if ctx.insn.instruction().operand_count_visible != 2 {
        ctx.missing_form();
        return None;
    }

    let operands = ctx.insn.operands();
    let lhs = operand(ctx, &operands[0]);
    let rhs = operand(ctx, &operands[1]);

    // An empty string means `format_operand` can't write this operand kind.
    if lhs.is_empty() || rhs.is_empty() {
        ctx.missing_form();
        return None;
    }
    Some((lhs, rhs))
}

fn operand(ctx: &Emitter<'_>, operand: &DecodedOperand) -> String {
    if let Some(expr) = platform_expr(ctx, operand) {
        return expr;
    }

    let suffix = operand_size_name(ctx.insn.operands()[0].size);
    ctx.format_operand(operand, suffix)
}

fn platform_expr(ctx: &Emitter<'_>, operand: &DecodedOperand) -> Option<String> {
    if ctx.insn.mnemonic() != Mnemonic::TEST {
        return None;
    }

    let mem = memory(operand).filter(|mem| mem.segment == Register::DS)?;
    let address = mem.disp.displacement as u32;
    match ctx.route(Register::DS, address, operand.size, false) {
        MemoryRoute::Expr(expr) => Some(expr),
        _ => None,
    }
}
