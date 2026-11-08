//! Code shared by the SSE math and conversion instructions.

use super::super::prelude::*;

pub(super) fn compare_ss(ctx: &mut Emitter<'_>, signalling: bool) {
    let Some((lhs, rhs)) = compare_operands(ctx) else {
        return ctx.missing_form();
    };
    // fast_sse_status skips the MXCSR exception flags.
    let status = if ctx.config.speed_hacks.runs_fast_sse_status() {
        ", false"
    } else {
        ""
    };
    cpp!(
        ctx.out,
        "ctx.compare_ss<{signalling}{status}>({lhs}, {rhs});"
    );
}

/// Gets the two scalar floats compared by COMISS or UCOMISS.
pub(in super::super) fn compare_operands(ctx: &Emitter<'_>) -> Option<(String, String)> {
    let operands = ctx.insn.operands();

    if usize::from(ctx.insn.instruction().operand_count) < 2 {
        return None;
    }

    let xmm_f32 = |reg| format!("_mm_cvtss_f32(ctx.xmm[{}].reg)", xmm_index(reg));

    // The context only stores XMM0 to XMM7.
    let lhs = register(&operands[0]).filter(|reg| is_xmm0_to_7(*reg))?;
    let rhs = if let Some(reg) = register(&operands[1]).filter(|reg| is_xmm0_to_7(*reg)) {
        xmm_f32(reg)
    } else if memory(&operands[1]).is_some() && operands[1].size == 32 {
        format!("X86_LOAD_F32({})", ctx.ea(&operands[1]))
    } else {
        return None;
    };

    Some((xmm_f32(lhs), rhs))
}

pub(super) fn float_s(ctx: &mut Emitter<'_>, sign: &str) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];
    let src = &operands[1];

    let func = match sign {
        "-" => "_mm_sub_ps",
        "*" => "_mm_mul_ps",
        _ => "_mm_add_ps",
    };

    let Some(dst_reg) = register(dst).filter(|reg| is_xmm0_to_7(*reg)) else {
        return ctx.missing_form();
    };
    let dst_idx = xmm_index(dst_reg);

    let src_expr = match (register(src), memory(src)) {
        // XMM register to XMM register
        (Some(reg), _) if is_xmm0_to_7(reg) => format!("ctx.xmm[{}].reg", xmm_index(reg)),
        (None, Some(_)) if src.size == 128 => {
            format!("{}LOAD_PS({})", packed_prefix(ctx), ctx.ea(src))
        }
        _ => return ctx.missing_form(),
    };
    cpp!(
        ctx.out,
        "ctx.xmm[{dst_idx}].reg = {func}(ctx.xmm[{dst_idx}].reg, {src_expr}); "
    );
}

pub(super) fn handle_cvtss2si(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let src = &operands[1];
    let ismm_cvttss_si32 = ctx.insn.mnemonic() == Mnemonic::CVTTSS2SI;
    let Some(dst) = register(&operands[0]).filter(|reg| reg.class() == RegisterClass::GPR32) else {
        return ctx.missing_form();
    };
    let dst_reg = get_x86_reg(dst);

    match (register(src), memory(src)) {
        (Some(src_reg), _) if is_xmm0_to_7(src_reg) => {
            let intrinsic = if ismm_cvttss_si32 {
                "_mm_cvttss_si32"
            } else {
                "_mm_cvtss_si32"
            };
            let src_reg = get_x86_reg_float(src_reg, ctx.fpu(), ctx.mmx());
            cpp!(ctx.out, "{dst_reg} = (int32_t){intrinsic}({src_reg});");
        }
        (None, Some(_)) => {
            let src_addr = ctx.ea(src);
            cpp!(
                ctx.out,
                "X86_CVTSS2SI_MEM({src_addr}, {dst_reg}, base, {ismm_cvttss_si32});"
            );
        }
        _ => ctx.missing_form(),
    }
}

pub(super) fn handle_sse_scalar_instruction(ctx: &mut Emitter<'_>, op_symbol: &'static str) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];
    let src = &operands[1];
    // Destination: XMM register.
    let Some(dst_reg) = register(dst).filter(|reg| is_xmm0_to_7(*reg)) else {
        return ctx.missing_form();
    };
    let dest = format!("ctx.xmm[{}].reg", xmm_index(dst_reg));

    // Source: XMM register or memory.
    let src_text = match (register(src), memory(src)) {
        (Some(reg), _) if is_xmm0_to_7(reg) => format!("ctx.xmm[{}].reg", xmm_index(reg)),
        (None, Some(_)) => format!("_mm_set_ss(X86_LOAD_F32({}))", ctx.ea(src)),
        _ => return ctx.missing_form(),
    };
    cpp!(ctx.out, "{dest} = {op_symbol}({dest}, {src_text});");
}

pub(super) fn packed_source(ctx: &Emitter<'_>, src: &DecodedOperand) -> Option<String> {
    match (register(src), memory(src)) {
        (Some(reg), _) if is_xmm0_to_7(reg) => Some(format!("ctx.xmm[{}].reg", xmm_index(reg))),
        (None, Some(_)) => Some(format!("{}LOAD_PS({})", packed_prefix(ctx), ctx.ea(src))),
        _ => None,
    }
}

/// Packed load/store macro prefix: `X86_PLAIN_` when `plain_sse_memory` runs.
pub(super) fn packed_prefix(ctx: &Emitter<'_>) -> &'static str {
    if ctx.config.speed_hacks.runs_plain_sse_memory() {
        "X86_PLAIN_"
    } else {
        "X86_"
    }
}

pub(super) fn xmm_register(operand: &DecodedOperand) -> Option<i64> {
    register(operand)
        .filter(|reg| is_xmm0_to_7(*reg))
        .map(xmm_index)
}
