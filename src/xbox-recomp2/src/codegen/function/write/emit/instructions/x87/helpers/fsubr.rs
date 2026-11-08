//! Code shared by `fsubr` and `fsubrp`.

use super::super::super::prelude::*;
use super::{emit_st0_exception_flags, stack_template_argument};

pub(in super::super) fn handle_fsubr(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let count = usize::from(ctx.insn.instruction().operand_count);
    let is_pop_version = ctx.insn.mnemonic() == Mnemonic::FSUBRP;
    let operand_ok = |op: &DecodedOperand| match (register(op), memory(op)) {
        (Some(reg), _) => reg.class() == RegisterClass::X87,
        (None, Some(_)) => matches!(op.size, 32 | 64),
        _ => false,
    };
    if count == 0 || !operands[..count.min(2)].iter().all(operand_ok) {
        return ctx.missing_form();
    }

    if count >= 2 {
        // Two operands: fsubr dst, src -> dst = src - dst
        if count == 3
            && memory(&operands[0]).is_some()
            && register(&operands[1]) == Some(Register::ST0)
            && operands[1].visibility == OperandVisibility::HIDDEN
        {
            emit_fsubr_memory_first(ctx, &operands[0]);
        } else {
            emit_fsubr_register_dst(ctx, &operands[0], &operands[1]);
        }
    } else if count == 1 {
        // One operand: fsubr src -> st(0) = src - st(0)
        emit_fsubr_single_operand(ctx, &operands[0]);
    }

    // Intel's manual leaves C0, C2 and C3 undefined after FSUBR, so we keep them.
    cpp!(ctx.out, "{fpu}.sw &= ~X87FPU::C1;");
    emit_st0_exception_flags(ctx);

    if is_pop_version {
        let checked = stack_template_argument(ctx);
        cpp!(ctx.out, "{fpu}.pop{checked}();");
    }
}

fn emit_fsubr_memory_first(ctx: &mut Emitter<'_>, src: &DecodedOperand) {
    let fpu = ctx.fpu();
    match u64::from(src.size) {
        32 => {
            let addr = ctx.ea(src);
            cpp!(ctx.out, "{fpu}.st0() = X86_LOAD_F32({addr}) - {fpu}.st0();");
        }
        64 => {
            let addr = ctx.ea(src);
            cpp!(ctx.out, "{fpu}.st0() = X86_LOAD_F64({addr}) - {fpu}.st0();");
        }
        _ => unreachable!("checked in handle_fsubr"),
    }
}

fn emit_fsubr_register_dst(ctx: &mut Emitter<'_>, dst: &DecodedOperand, src: &DecodedOperand) {
    let fpu = ctx.fpu();
    let dst_is_x87_reg = register(dst).is_some_and(|reg| reg.class() == RegisterClass::X87);
    if !dst_is_x87_reg {
        return;
    }

    let dst_reg = register(dst).unwrap();
    if let Some(src_reg) = register(src) {
        if src_reg.class() == RegisterClass::X87 {
            // fsubr st(i), st(j) -> st(i) = st(j) - st(i)
            let dst_idx = st_index(dst_reg);
            let src_idx = st_index(src_reg);
            cpp!(
                ctx.out,
                "{fpu}.st{dst_idx}() = {fpu}.st{src_idx}() - {fpu}.st{dst_idx}();"
            );
        }
    } else if memory(src).is_some() {
        // fsubr st(i), [mem] -> st(i) = mem - st(i)
        match u64::from(src.size) {
            32 => {
                let dst_idx = st_index(dst_reg);
                let addr = ctx.ea(src);
                cpp!(
                    ctx.out,
                    "{fpu}.st{dst_idx}() = X86_LOAD_F32({addr}) - {fpu}.st{dst_idx}();"
                );
            }
            64 => {
                let dst_idx = st_index(dst_reg);
                let addr = ctx.ea(src);
                cpp!(
                    ctx.out,
                    "{fpu}.st{dst_idx}() = X86_LOAD_F64({addr}) - {fpu}.st{dst_idx}();"
                );
            }
            _ => unreachable!("checked in handle_fsubr"),
        }
    }
}

fn emit_fsubr_single_operand(ctx: &mut Emitter<'_>, src: &DecodedOperand) {
    let fpu = ctx.fpu();
    if let Some(src_reg) = register(src) {
        if src_reg.class() == RegisterClass::X87 {
            let src_idx = st_index(src_reg);
            cpp!(ctx.out, "{fpu}.st0() = {fpu}.st{src_idx}() - {fpu}.st0();");
        } else {
            let src_text = get_x86_reg(src_reg);
            cpp!(
                ctx.out,
                "{fpu}.st0() = (x87_float){src_text} - {fpu}.st0();"
            );
        }
    } else if memory(src).is_some() {
        match u64::from(src.size) {
            32 => {
                // FSUBR: result = memory_value - ST(0)
                let addr = ctx.ea(src);
                cpp!(ctx.out, "{fpu}.st0() = X86_LOAD_F32({addr}) - {fpu}.st0();");
            }
            64 => {
                // FSUBR: result = memory_value - ST(0)
                let addr = ctx.ea(src);
                cpp!(ctx.out, "{fpu}.st0() = X86_LOAD_F64({addr}) - {fpu}.st0();");
            }
            _ => {
                if ctx.config.debug {
                    cpp!(
                        ctx.out,
                        "assert(false && \"Invalid operand size for FSUBR\");"
                    );
                }
            }
        }
    }
}
