//! Read decoded guest instructions.

use super::{
    DecodedInstruction, DecodedOperand, DecodedOperandKind, ImmediateInfo, MemoryInfo, Mnemonic,
    Register,
};

/// Fewest operands a clone keeps.
///
/// Some code reads `operands()[1]` and `[2]` without checking the count.
const MIN_CLONED_OPERANDS: usize = 3;

/// A decoded instruction.
///
/// # Operands
///
/// `mov eax, 0x11223344` fills two slots: a register and an immediate.
/// - eax (register)
/// - 0x11223344 (immediate)
///
/// The other slots default to [`DecodedOperandKind::Unused`].
#[derive(Debug, PartialEq, Eq)]
pub struct Insn {
    /// The instruction.
    pub(super) instruction: DecodedInstruction,

    /// The operands.
    pub(super) operands: Box<[DecodedOperand]>,
}

impl Insn {
    /// Returns the decoded instruction.
    #[inline]
    pub fn instruction(&self) -> &DecodedInstruction {
        &self.instruction
    }

    /// Returns the operands. Some slots may be unused.
    #[inline]
    pub fn operands(&self) -> &[DecodedOperand] {
        &self.operands
    }

    /// Returns the instruction mnemonic.
    #[inline]
    pub fn mnemonic(&self) -> Mnemonic {
        self.instruction.mnemonic
    }

    /// Returns the instruction length in bytes.
    #[inline]
    pub fn length(&self) -> usize {
        usize::from(self.instruction.length)
    }

    /// Returns operand `index`'s memory details for editing.
    ///
    /// Returns [`None`] if that operand isn't memory.
    pub(crate) fn memory_mut(&mut self, index: usize) -> Option<&mut MemoryInfo> {
        match &mut self.operands[index].kind {
            DecodedOperandKind::Mem(mem) => Some(mem),
            _ => None,
        }
    }
}

impl Clone for Insn {
    fn clone(&self) -> Self {
        let kept = usize::from(self.instruction.operand_count)
            .max(MIN_CLONED_OPERANDS)
            .min(self.operands.len());
        Self {
            instruction: self.instruction.clone(),
            operands: self.operands[..kept].into(),
        }
    }
}

/// Reads an immediate operand's value as [`i64`].
///
/// # Arguments
///
/// * `operand` - the operand to read.
///
/// # Returns
///
/// Immediate operand, [`None`] otherwise.
#[inline]
pub fn immediate_signed(operand: &DecodedOperand) -> Option<i64> {
    immediate(operand).map(|imm| imm.value as i64)
}

/// Gets the memory details of an operand.
///
/// # Arguments
///
/// * `operand` - the operand to read.
///
/// # Returns
///
/// The [`MemoryInfo`] for memory operand, [`None`] otherwise.
#[inline]
pub fn memory(operand: &DecodedOperand) -> Option<&MemoryInfo> {
    match &operand.kind {
        DecodedOperandKind::Mem(mem) => Some(mem),
        _ => None,
    }
}

/// Gets the register of an operand.
///
/// # Arguments
///
/// * `operand` - the operand to read.
///
/// # Returns
///
/// The [`Register`] for register operand, [`None`] otherwise.
#[inline]
pub fn register(operand: &DecodedOperand) -> Option<Register> {
    match &operand.kind {
        DecodedOperandKind::Reg(reg) => Some(*reg),
        _ => None,
    }
}

/// Gets the immediate details of an operand.
///
/// # Arguments
///
/// * `operand` - the operand to read.
///
/// # Returns
///
/// The [`ImmediateInfo`] for immediate operand, [`None`] otherwise.
#[inline]
pub fn immediate(operand: &DecodedOperand) -> Option<&ImmediateInfo> {
    match &operand.kind {
        DecodedOperandKind::Imm(imm) => Some(imm),
        _ => None,
    }
}
