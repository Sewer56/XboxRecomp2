//! Checks whether an instruction reads or writes a register.
//!
//! Wrap an instruction with [`RegisterAccess::of`], then ask:
//!
//! - [`reads`]: is it read?
//! - [`reads_value`]: is its value read?
//! - [`writes`]: is any part of it written?
//! - [`touches`]: is it read or written?
//!
//! [`reads`]: RegisterAccess::reads
//! [`reads_value`]: RegisterAccess::reads_value
//! [`writes`]: RegisterAccess::writes
//! [`touches`]: RegisterAccess::touches

use crate::decode::{DecodedOperand, Insn, MachineMode, OperandAction, Register, memory, register};

/// Answers register read/write questions for one instruction.
pub(in crate::codegen) struct RegisterAccess<'a>(&'a Insn);

impl<'a> RegisterAccess<'a> {
    /// Wraps `insn`.
    pub(in crate::codegen) fn of(insn: &'a Insn) -> Self {
        Self(insn)
    }

    /// Whether the instruction reads any part of `target`.
    ///
    /// For EAX:
    /// - `push eax`: yes.
    /// - `mov ecx, [eax]`: yes.
    /// - `mov cl, ah`: yes.
    /// - `mov eax, ebx`: no.
    pub(in crate::codegen) fn reads(&self, target: Register) -> bool {
        // Is `target` used in an address, like `[eax]` or `[ecx + eax*4]`?
        for operand in self.operands() {
            if let Some(mem) = memory(operand)
                && (overlaps(mem.base, target) || overlaps(mem.index, target))
            {
                return true;
            }
        }

        self.reads_value(target)
    }

    /// Whether the instruction reads any part of `target` as a value.
    ///
    /// For EAX:
    /// - `push eax`: yes.
    /// - `mov ecx, [eax]`: no.
    pub(in crate::codegen) fn reads_value(&self, target: Register) -> bool {
        self.has_register_operand(target, OperandAction::READ | OperandAction::CONDREAD)
    }

    /// Whether the instruction may write any part of `target`.
    ///
    /// - `mov ah, 1`: writes EAX.
    /// - `mul ebx`: writes EDX (hidden operand).
    /// - `cmovz eax, ebx`: writes EAX, even if the move doesn't happen.
    pub(in crate::codegen) fn writes(&self, target: Register) -> bool {
        self.has_register_operand(target, OperandAction::WRITE | OperandAction::CONDWRITE)
    }

    /// Whether the instruction reads or writes any part of `target`.
    ///
    /// Same as [`Self::reads`] or [`Self::writes`].
    pub(in crate::codegen) fn touches(&self, target: Register) -> bool {
        self.reads(target) || self.writes(target)
    }

    /// Whether a register operand overlapping `target` has any of `actions`.
    fn has_register_operand(&self, target: Register, actions: OperandAction) -> bool {
        // Get wrapped instruction ops
        for operand in self.operands() {
            // Check if action matches first.
            if !operand.action.intersects(actions) {
                continue;
            }

            // size check, e.g. AH + EAX
            if let Some(reg) = register(operand)
                && overlaps(reg, target)
            {
                return true;
            }
        }

        false
    }

    /// The instruction's operands.
    fn operands(&self) -> &[DecodedOperand] {
        let count = usize::from(self.0.instruction().operand_count);
        &self.0.operands()[..count]
    }
}

/// Whether two registers share any bits, like `AL` and `EAX`.
fn overlaps(left: Register, right: Register) -> bool {
    if left == Register::NONE || right == Register::NONE {
        return false;
    }

    left.largest_enclosing(MachineMode::LEGACY_32)
        == right.largest_enclosing(MachineMode::LEGACY_32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::instruction;
    use rstest::rstest;

    /// Each case gives `(reads, reads_value, writes)` for `target`.
    #[rstest]
    #[case::push(&[0x50], Register::EAX, (true, true, false))] // push eax
    #[case::address(&[0x8B, 0x08], Register::EAX, (true, false, false))] // mov ecx, [eax]
    #[case::lea(&[0x8D, 0x08], Register::EAX, (true, false, false))] // lea ecx, [eax]
    #[case::partial_read(&[0x88, 0xE1], Register::EAX, (true, true, false))] // mov cl, ah
    #[case::partial_write(&[0xB4, 1], Register::EAX, (false, false, true))] // mov ah, 1
    #[case::implicit(&[0xF7, 0xE3], Register::EDX, (false, false, true))] // mul ebx
    #[case::conditional(&[0x0F, 0x44, 0xC3], Register::EAX, (false, false, true))] // cmovz eax, ebx
    #[case::unrelated(&[0x89, 0xD8], Register::ECX, (false, false, false))] // mov eax, ebx
    fn accesses_should_include_hidden_and_overlapping_registers(
        #[case] code: &[u8],
        #[case] target: Register,
        #[case] expected: (bool, bool, bool),
    ) {
        // Arrange.
        let insn = instruction(code);

        // Act.
        let access = RegisterAccess::of(&insn);
        let actual = (
            access.reads(target),
            access.reads_value(target),
            access.writes(target),
        );

        // Assert.
        assert_eq!(actual, expected);
    }
}
