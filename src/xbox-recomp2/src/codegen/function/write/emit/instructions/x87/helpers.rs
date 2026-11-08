//! Code shared by the x87 math and compare instructions.

use super::super::merged_cmp_jmp::CompareOutput;
use super::super::prelude::*;
use crate::decode::CpuFlag;
pub(super) use fsubr::handle_fsubr;

mod fsubr;

pub(super) fn float_operations(ctx: &mut Emitter<'_>, sign: &'static str, op_size: u16) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let count = usize::from(ctx.insn.instruction().operand_count);

    let operand_ok = |op: &DecodedOperand| match (register(op), memory(op)) {
        (Some(reg), _) => reg.class() == RegisterClass::X87,
        (None, Some(_)) => matches!(op.size, 32 | 64),
        _ => false,
    };
    let explicit = operands[..count]
        .iter()
        .filter(|op| op.visibility != OperandVisibility::HIDDEN);
    if count == 0 || !explicit.clone().all(operand_ok) || explicit.count() == 0 {
        return ctx.missing_form();
    }

    let result_in_st0 = if count >= 2 {
        emit_arith_two_operands(ctx, operands, count, sign, op_size)
    } else {
        emit_arith_single_operand(ctx, &operands[0], sign, op_size);
        true
    };

    // Intel's manual leaves C0, C2 and C3 undefined after FADD, FSUB, FMUL
    // and FDIV, so we keep them.
    cpp!(ctx.out, "{fpu}.sw &= ~X87FPU::C1;");
    emit_precision_flag(ctx);
    if result_in_st0 {
        emit_exception_flags(ctx, "val_double_0");
    } else {
        emit_st0_exception_flags(ctx);
    }

    if sign == "/" && ctx.config.debug {
        emit_zero_divide_report(ctx);
    }
}

pub(super) fn handle_fcom(ctx: &mut Emitter<'_>, pop_count: i64) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let status = status_stack_template_argument(ctx);

    if memory(&operands[0]).is_some() {
        if !matches!(operands[0].size, 32 | 64) {
            ctx.missing_form();
        } else if operands[0].size == 32 {
            let addr = ctx.ea(&operands[0]);
            cpp!(
                ctx.out,
                "{fpu}.compare_and_pop{status}(X86_LOAD_F32({addr}), {pop_count});"
            );
        } else {
            let addr = ctx.ea(&operands[0]);
            cpp!(
                ctx.out,
                "{fpu}.compare_and_pop{status}(X86_LOAD_F64({addr}), {pop_count});"
            );
        }
    } else if let Some(reg) = register(&operands[0]) {
        let st_idx = get_st_index(reg);
        cpp!(
            ctx.out,
            "{fpu}.compare_and_pop{status}({fpu}.st{st_idx}(), {pop_count});"
        );
    } else {
        ctx.missing_form();
    }
}

/// FCOMIP, or FUCOMIP when `ordered` is false (it signals only on signalling NaNs).
pub(super) fn handle_fcomi(ctx: &mut Emitter<'_>, ordered: bool) {
    let operands = ctx.insn.operands();
    let count = usize::from(ctx.insn.instruction().operand_count);

    if count >= 2 {
        emit_compare_and_set_eflags(ctx, &operands[1], ordered);
    } else if count == 1 {
        emit_compare_and_set_eflags(ctx, &operands[0], ordered);
    }
}

pub(super) fn handle_fdivr(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let is_pop_version = ctx.insn.mnemonic() == Mnemonic::FDIVRP;
    let base = ctx.base.get();

    if usize::from(ctx.insn.instruction().operand_count) == 3 {
        let dst_reg = register(&operands[0]);
        let src_reg = register(&operands[1]);
        let register_form = match (dst_reg, src_reg) {
            (Some(dst), Some(src)) => is_st0_to_7(dst) && is_st0_to_7(src),
            _ => false,
        };

        if register_form {
            let st_dest = st_index(dst_reg.unwrap());
            let st_src = st_index(src_reg.unwrap());

            cpp!(
                ctx.out,
                "{fpu}.st{st_dest}() = X87_FDIVR({fpu}.st{st_dest}(), {fpu}.st{st_src}(), {fpu}.st{st_dest}(), 0x{base:X});"
            );
        } else if memory(&operands[0]).is_some() {
            match u64::from(operands[0].size) {
                32 => {
                    let addr = ctx.ea(&operands[0]);
                    cpp!(
                        ctx.out,
                        "{fpu}.st0() = X87_FDIVR({fpu}.st0(), X86_LOAD_F32({addr}), {fpu}.st0(), 0x{base:X});"
                    );
                }
                64 => {
                    let addr = ctx.ea(&operands[0]);
                    cpp!(
                        ctx.out,
                        "{fpu}.st0() = X87_FDIVR({fpu}.st0(), X86_LOAD_F64({addr}), {fpu}.st0(), 0x{base:X});"
                    );
                }
                _ => return ctx.missing_form(),
            }
        } else {
            return ctx.missing_form();
        }
    } else {
        return ctx.missing_form();
    }

    if is_pop_version {
        let checked = stack_template_argument(ctx);
        cpp!(ctx.out, "{fpu}.pop{checked}();");
    }
    cpp!(ctx.out, "{fpu}.sw &= ~X87FPU::C1;");
}

pub(super) fn emit_precision_flag(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    if !ctx.config.speed_hacks.runs_fast_x87_status() {
        cpp!(ctx.out, "{fpu}.set_precision_flag();");
    }
}

pub(super) fn precision_flag_line(ctx: &Emitter<'_>) -> String {
    if ctx.config.speed_hacks.runs_fast_x87_status() {
        String::new()
    } else {
        format!("\n    {}.set_precision_flag();", ctx.fpu())
    }
}

pub(super) fn status_template_argument(ctx: &Emitter<'_>) -> &'static str {
    if ctx.config.speed_hacks.runs_fast_x87_status() {
        "<false>"
    } else {
        ""
    }
}

pub(super) fn status_stack_template_argument(ctx: &Emitter<'_>) -> &'static str {
    let hacks = ctx.config.speed_hacks;
    match (hacks.runs_fast_x87_status(), hacks.runs_fast_x87_stack()) {
        (false, false) => "",
        (true, false) => "<false>",
        (false, true) => "<true, false>",
        (true, true) => "<false, false>",
    }
}

pub(super) fn stack_template_argument(ctx: &Emitter<'_>) -> &'static str {
    if ctx.config.speed_hacks.runs_fast_x87_stack() {
        "<false>"
    } else {
        ""
    }
}

pub(super) fn emit_exception_flags(ctx: &mut Emitter<'_>, result: &str) {
    let fpu = ctx.fpu();
    if !ctx.config.speed_hacks.runs_fast_x87_status() {
        cpp!(ctx.out, "{fpu}.set_exception_flags({result});");
    }
}

/// Like [`emit_exception_flags`] with ST(0) as the result.
pub(super) fn emit_st0_exception_flags(ctx: &mut Emitter<'_>) {
    if !ctx.config.speed_hacks.runs_fast_x87_status() {
        let fpu = ctx.fpu();
        cpp!(ctx.out, "{fpu}.set_exception_flags({fpu}.st0());");
    }
}

#[inline]
pub(super) fn get_st_index(reg: Register) -> u8 {
    let index = (reg as u16).wrapping_sub(Register::ST0 as u16) as u8;
    assert!(index < 8);
    index
}

fn emit_arith_single_operand(
    ctx: &mut Emitter<'_>,
    src: &DecodedOperand,
    sign: &'static str,
    op_size: u16,
) {
    let fpu = ctx.fpu();
    if let Some(src_reg) = register(src) {
        if src_reg.class() == RegisterClass::X87 {
            emit_st_binop(ctx, 0, src_reg, sign);
        } else {
            // Integer register, converted to double.
            let src_text = get_x86_reg(src_reg);
            cpp!(
                ctx.out,
                "val_double_0 = {fpu}.st0() {sign} (double){src_text};"
            );
        }
    } else if memory(src).is_some() {
        if op_size == 32 || op_size == 64 {
            emit_val_double_mem_op(ctx, 0, sign, op_size, src);
        } else if ctx.config.debug {
            let base = ctx.base.get();
            cpp!(
                ctx.out,
                "printf(\"FPU_EXCEPTION FLOATOPERATIONS FUNCTION {base:X}\\n\");"
            );
        }
    }

    // With one operand, the result goes to ST(0).
    cpp!(ctx.out, "{fpu}.st0() = val_double_0;");
}

fn emit_arith_two_operands(
    ctx: &mut Emitter<'_>,
    operands: &[DecodedOperand],
    count: usize,
    sign: &'static str,
    op_size: u16,
) -> bool {
    let fpu = ctx.fpu();
    // Memory source: Zydis lists it first, then a hidden ST(0).
    if count == 3
        && memory(&operands[0]).is_some()
        && register(&operands[1]) == Some(Register::ST0)
        && operands[1].visibility == OperandVisibility::HIDDEN
    {
        emit_val_double_mem_op(ctx, 0, sign, op_size, &operands[0]);

        cpp!(ctx.out, "{fpu}.st0() = val_double_0;");
        return true;
    }

    let dst = &operands[0];
    let src = &operands[1];

    let dst_is_x87_reg = register(dst).is_some_and(|reg| reg.class() == RegisterClass::X87);
    let dst_is_st0_or_implicit =
        register(dst) == Some(Register::ST0) || dst.visibility == OperandVisibility::IMPLICIT;

    // ST(i) destination.
    if dst_is_x87_reg {
        let dst_reg = register(dst).unwrap();
        let dst_idx = st_index(dst_reg);

        if let Some(src_reg) = register(src) {
            if src_reg.class() == RegisterClass::X87 {
                emit_st_binop(ctx, dst_idx, src_reg, sign);
            }
        } else if memory(src).is_some() {
            emit_val_double_mem_op(ctx, dst_idx, sign, op_size, src);
        }
        cpp!(ctx.out, "{fpu}.st{dst_idx}() = val_double_0;");
        dst_idx == 0
    } else if dst_is_st0_or_implicit {
        // Implicit ST(0) destination.
        if let Some(src_reg) = register(src) {
            if src_reg.class() == RegisterClass::X87 {
                emit_st_binop(ctx, 0, src_reg, sign);
            }
        } else if memory(src).is_some() {
            emit_val_double_mem_op(ctx, 0, sign, op_size, src);
        }
        cpp!(ctx.out, "{fpu}.st0() = val_double_0;");
        true
    } else {
        false
    }
}

/// Writes an FCOMIP or FUCOMIP merged with the jumps and selects after it.
///
/// # Arguments
///
/// - `tested`: flags every jump and select tests, like CF for `jb`.
///   [`None`] when they test different flags.
/// - `pop_after`: whether an `fstp st0` follows the compare.
///
/// # Returns
///
/// - [`CompareOutput::Float`] with `fcomi_taken` when they test the same flags.
/// - [`CompareOutput::Flags`] when they don't: `fcomi_flags`, or eflags when
///   tracking status flags.
///
/// # Examples
///
/// `fcomip st0, st1; fstp st0; jb below` writes:
///
/// ```cpp
/// val_double_0 = ctx.fpu.st1();
/// const bool fcomi_taken = ctx.fpu.branch_fcomi<true, 0x1>(ctx.fpu.st0(), val_double_0, ctx.eflags);
/// ctx.fpu.pop(); // <- from fcomip
/// ctx.fpu.pop(); // <- from fstp, only when `pop_after` is set
/// ```
///
/// The caller then writes `if (fcomi_taken) goto below;`.
///
/// With `jb` and `jz` after it, and `fast_x87_status`, the compare line is instead:
///
/// ```cpp
/// const uint32_t fcomi_flags = x86_float_flags(ctx.fpu.st0(), val_double_0);
/// ```
pub(in super::super) fn merged_fcomi(
    ctx: &mut Emitter<'_>,
    tested: Option<CpuFlag>,
    pop_after: bool,
) -> CompareOutput {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let src = if usize::from(ctx.insn.instruction().operand_count) >= 2 {
        &operands[1]
    } else {
        &operands[0]
    };
    let st_idx = st_index(operand_reg_value(src));

    let status = !ctx.config.speed_hacks.runs_fast_x87_status();
    let unordered = if ctx.insn.mnemonic() == Mnemonic::FCOMIP {
        ""
    } else {
        ", false"
    };
    let checked = stack_template_argument(ctx);

    // Compare, then pop.
    cpp!(ctx.out, "val_double_0 = {fpu}.st{st_idx}();");
    let output = match tested {
        Some(mask) => {
            let mask = mask.bits();
            cpp!(
                ctx.out,
                "const bool fcomi_taken = {fpu}.branch_fcomi<{status}, 0x{mask:X}>({fpu}.st0(), val_double_0, ctx.eflags{unordered});"
            );
            CompareOutput::Float("fcomi_taken".to_owned())
        }
        // With status flags, clang does as well with plain eflags as with a local.
        None if status => {
            cpp!(
                ctx.out,
                "{fpu}.compare_and_set_eflags({fpu}.st0(), val_double_0, ctx.eflags{unordered});"
            );
            CompareOutput::Flags("ctx.eflags.common".to_owned())
        }
        None => {
            cpp!(
                ctx.out,
                "const uint32_t fcomi_flags = x86_float_flags({fpu}.st0(), val_double_0);"
            );
            CompareOutput::Flags("fcomi_flags".to_owned())
        }
    };
    cpp!(ctx.out, "{fpu}.pop{checked}();");
    if pop_after {
        cpp!(ctx.out, "{fpu}.pop{checked}();");
    }
    output
}

fn emit_compare_and_set_eflags(ctx: &mut Emitter<'_>, src: &DecodedOperand, ordered: bool) {
    let fpu = ctx.fpu();
    let st_idx = st_index(operand_reg_value(src));
    let status = status_template_argument(ctx);
    let unordered = if ordered { "" } else { ", false" };
    let checked = stack_template_argument(ctx);

    cpp!(
        ctx.out,
        "
        val_double_0 = {fpu}.st{st_idx}();
        {fpu}.compare_and_set_eflags{status}({fpu}.st0(), val_double_0, ctx.eflags{unordered});
        {fpu}.pop{checked}();
    "
    );
}

fn emit_zero_divide_report(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let base = ctx.base.get();
    cpp!(
        ctx.out,
        "
        if (!std::isfinite(val_double_0) && val_double_0 == {fpu}.st0() / 0.0) {{
            printf(\"FLOATOPERATIONS ZERO DIVIDE {base:X}\\n\");
        }}
    "
    );
}

fn emit_st_binop(ctx: &mut Emitter<'_>, dst_idx: i64, src_reg: Register, sign: &'static str) {
    let fpu = ctx.fpu();
    let src_idx = st_index(src_reg);
    cpp!(
        ctx.out,
        "val_double_0 = {fpu}.st{dst_idx}() {sign} {fpu}.st{src_idx}();"
    );
}

fn emit_val_double_mem_op(
    ctx: &mut Emitter<'_>,
    dest_stack: i64,
    sign: &'static str,
    width: u16,
    src: &DecodedOperand,
) {
    let fpu = ctx.fpu();
    let load = if width == 32 {
        "X86_LOAD_F32("
    } else if width == 64 {
        "X86_LOAD_F64("
    } else {
        return;
    };

    let addr = ctx.ea(src);
    cpp!(
        ctx.out,
        "val_double_0 = {fpu}.st{dest_stack}() {sign} {load}{addr});"
    );
}
