//! Moves, pushes and pops, conditional moves and `set*`.

pub(super) mod cmovb;
pub(super) mod cmovbe;
pub(super) mod cmovl;
pub(super) mod cmovle;
pub(super) mod cmovnb;
pub(super) mod cmovnbe;
pub(super) mod cmovnl;
pub(super) mod cmovnle;
pub(super) mod cmovno;
pub(super) mod cmovnp;
pub(super) mod cmovns;
pub(super) mod cmovnz;
pub(super) mod cmovo;
pub(super) mod cmovp;
pub(super) mod cmovs;
pub(super) mod cmovz;
pub(super) mod cmpxchg;
pub(super) mod lea;
pub(super) mod leave;
pub(super) mod mov;
pub(super) mod movsx;
pub(super) mod movzx;
pub(super) mod pop;
pub(super) mod push;
pub(super) mod pushfd;
pub(super) mod setb;
pub(super) mod setbe;
pub(super) mod setl;
pub(super) mod setle;
pub(super) mod setnb;
pub(super) mod setnbe;
pub(super) mod setnl;
pub(super) mod setnle;
pub(super) mod setno;
pub(super) mod setnp;
pub(super) mod setns;
pub(super) mod setnz;
pub(super) mod seto;
pub(super) mod setp;
pub(super) mod sets;
pub(super) mod setz;
pub(super) mod xchg;

use super::prelude::{
    Emitter, Register, RegisterClass, cpp, ea, get_x86_reg, memory, operand_size_name,
    operand_type_name, register, segment_equals,
};
use zydis::{InstructionCategory, MachineMode};

/// Writes a SETcc or CMOVcc on the C++ test `condition`.
pub(in super::super) fn write_conditional(ctx: &mut Emitter<'_>, condition: &str) {
    if ctx.insn.instruction().meta.category == InstructionCategory::SETCC {
        write_setcc(ctx, condition);
    } else {
        write_cmov(ctx, condition);
    }
}

/// Writes a CMOVcc that moves when the C++ test `condition` is true.
fn write_cmov(ctx: &mut Emitter<'_>, condition: &str) {
    let operands = ctx.insn.operands();
    let (Some(dst), Some(src)) = (operands.first(), operands.get(1)) else {
        ctx.missing_form();
        return;
    };
    let Some(dst_reg) = register(dst) else {
        ctx.missing_form();
        return;
    };
    let dst_str = get_x86_reg(dst_reg);

    // A register source is only read when the move happens.
    if let Some(src_reg) = register(src) {
        let src_str = get_x86_reg(src_reg);
        cpp!(
            ctx.out,
            "
            {dst_str} = ({condition}) ? {src_str} : {dst_str};"
        );
        return;
    }

    // The fs and gs segments need the platform's routing, which CMOVcc lacks.
    if memory(src).is_none()
        || segment_equals(src, Register::FS)
        || segment_equals(src, Register::GS)
    {
        ctx.missing_form();
        return;
    }
    let load = format!("X86_LOAD_{}({})", operand_size_name(dst.size), ea(src));
    if ctx.config.speed_hacks.runs_lazy_cmov_load() {
        cpp!(
            ctx.out,
            "
            if ({condition}) {{
                {dst_str} = {load};
            }}"
        );
        return;
    }
    let src_type = operand_type_name(dst.size);
    cpp!(
        ctx.out,
        "
        {{
            const {src_type} src = {load};
            {dst_str} = ({condition}) ? src : {dst_str};
        }}"
    );
}

/// Writes a SETcc that stores the C++ test `condition` as 0 or 1.
fn write_setcc(ctx: &mut Emitter<'_>, condition: &str) {
    let Some(dst) = ctx.insn.operands().first() else {
        ctx.missing_form();
        return;
    };
    if let Some(dst_reg) = register(dst) {
        write_register(ctx, dst_reg, &format!("{condition} ? 1 : 0"));
    } else if memory(dst).is_some() {
        let dst_ea = ea(dst);
        cpp!(ctx.out, "X86_STORE_U8({dst_ea}, {condition} ? 1 : 0);");
    } else {
        ctx.missing_form();
    }
}

/// Writes the assignment of `value` to the general-purpose register `dst`.
fn write_register(ctx: &mut Emitter<'_>, dst: Register, value: &str) {
    let assignment = register_assignment(dst, value);
    cpp!(ctx.out, "{assignment};");
}

/// Builds the assignment of `value` to the general-purpose register `dst`.
/// 8-bit and 16-bit writes go through the 32-bit register, which is faster.
///
/// # Arguments
///
/// - `dst`: destination register.
/// - `value`: the new value, of the destination's width.
///
/// # Returns
///
/// The assignment without its `;`, for example:
///
/// ```text
/// ctx.eax.u32 = (ctx.eax.u32 & 0xFFFF00FFu) | ((uint32_t)(uint8_t)(0x5u) << 8)
/// ```
pub(super) fn register_assignment(dst: Register, value: &str) -> String {
    if !merges_into_parent(dst) {
        return format!("{} = {value}", get_x86_reg(dst));
    }

    let parent = get_x86_reg(dst.largest_enclosing(MachineMode::LONG_COMPAT_32));
    if dst.class() == RegisterClass::GPR16 {
        format!("{parent} = ({parent} & 0xFFFF0000u) | (uint16_t)({value})")
    } else if matches!(
        dst,
        Register::AH | Register::CH | Register::DH | Register::BH
    ) {
        format!("{parent} = ({parent} & 0xFFFF00FFu) | ((uint32_t)(uint8_t)({value}) << 8)")
    } else {
        format!("{parent} = ({parent} & 0xFFFFFF00u) | (uint8_t)({value})")
    }
}

/// Returns whether [`register_assignment`] writes `dst` via its 32-bit register.
pub(super) fn merges_into_parent(dst: Register) -> bool {
    matches!(dst.class(), RegisterClass::GPR8 | RegisterClass::GPR16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── Register destinations ────────────────────────────────────────────

    #[rstest]
    #[case::low_byte(
        Register::CL,
        "ctx.ecx.u32 = (ctx.ecx.u32 & 0xFFFFFF00u) | (uint8_t)(v)"
    )]
    #[case::high_byte(
        Register::DH,
        "ctx.edx.u32 = (ctx.edx.u32 & 0xFFFF00FFu) | ((uint32_t)(uint8_t)(v) << 8)"
    )]
    #[case::word(
        Register::SI,
        "ctx.esi.u32 = (ctx.esi.u32 & 0xFFFF0000u) | (uint16_t)(v)"
    )]
    #[case::dword(Register::EAX, "ctx.eax.u32 = v")]
    fn register_assignment_should_merge_only_bytes_and_words(
        #[case] dst: Register,
        #[case] expected: &str,
    ) {
        // Act
        let assignment = register_assignment(dst, "v");

        // Assert
        assert_eq!(assignment, expected);
    }
}
