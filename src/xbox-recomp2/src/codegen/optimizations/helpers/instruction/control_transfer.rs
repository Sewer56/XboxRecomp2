//! Spots instructions that can go somewhere other than the next instruction.

use crate::decode::{Insn, InstructionCategory};

/// Whether `insn` is a jump, call, return, interrupt or system instruction.
///
/// e.g. `jecxz`, `loop`, `jmp`, `call`, `ret` or `int`.
pub(in crate::codegen) fn leaves_block(insn: &Insn) -> bool {
    matches!(
        insn.instruction().meta.category,
        InstructionCategory::COND_BR
            | InstructionCategory::UNCOND_BR
            | InstructionCategory::CALL
            | InstructionCategory::RET
            | InstructionCategory::INTERRUPT
            | InstructionCategory::SYSCALL
            | InstructionCategory::SYSRET
            | InstructionCategory::SYSTEM
    )
}
