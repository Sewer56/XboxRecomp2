//! Spots simple instructions, like `add` or `mov`, that leave ESP alone.

use super::register_access::RegisterAccess;
use crate::decode::{
    DecodedOperand, Insn, Mnemonic, OperandAction, Register, RegisterClass, memory, register,
};

/// Whether `insn` is a recognised instruction that leaves ESP alone.
pub(in crate::codegen) fn leaves_esp_alone(insn: &Insn, tracked_write: bool) -> bool {
    // Only instructions we know well.
    if !matches!(
        insn.mnemonic(),
        Mnemonic::MOV
            | Mnemonic::MOVSS
            | Mnemonic::LEA
            | Mnemonic::NOP
            | Mnemonic::ADD
            | Mnemonic::ADC
            | Mnemonic::SUB
            | Mnemonic::SBB
            | Mnemonic::AND
            | Mnemonic::OR
            | Mnemonic::XOR
            | Mnemonic::SHL
            | Mnemonic::SHR
            | Mnemonic::SAR
            | Mnemonic::INC
            | Mnemonic::DEC
            | Mnemonic::NEG
            | Mnemonic::NOT
            | Mnemonic::IMUL
            | Mnemonic::TEST
            | Mnemonic::CMP
            | Mnemonic::MOVZX
            | Mnemonic::MOVSX
            | Mnemonic::ADDSS
            | Mnemonic::SUBSS
            | Mnemonic::MULSS
            | Mnemonic::DIVSS
            | Mnemonic::CVTSI2SS
            | Mnemonic::XORPS
            | Mnemonic::MOVAPS
            | Mnemonic::FLD1
            | Mnemonic::FLDZ
    ) || RegisterAccess::of(insn).writes(Register::ESP)
    {
        return false;
    }

    // Hidden operands, like the EDX in `mul ebx`, aren't checked.
    let visible = usize::from(insn.instruction().operand_count_visible);
    insn.operands()[..visible]
        .iter()
        .all(|operand| operand_is_simple(operand, tracked_write))
}

/// Whether we can step over one operand.
///
/// - Registers: general-purpose or XMM only.
/// - Memory: DS or SS only.
/// - Anything else, like an immediate, is fine.
fn operand_is_simple(operand: &DecodedOperand, tracked_write: bool) -> bool {
    // Like `eax`, `cl` or `xmm0`.
    if let Some(reg) = register(operand) {
        return matches!(
            reg.class(),
            RegisterClass::GPR8 | RegisterClass::GPR16 | RegisterClass::GPR32 | RegisterClass::XMM
        );
    }

    // Like `[ecx]` or `[esp+4]`.
    if let Some(mem) = memory(operand) {
        if !matches!(mem.segment, Register::DS | Register::SS) {
            return false;
        }

        // Reads are fine, like `cmp [mem], eax`.
        // Writes, like `add [mem], eax`, need the caller to have handled them.
        let writes = operand
            .action
            .intersects(OperandAction::WRITE | OperandAction::CONDWRITE);
        return tracked_write || !writes;
    }

    // An immediate, like the `4` in `add eax, 4`.
    true
}
