//! x87 related helpers.

use crate::decode::{Insn, Mnemonic, OperandVisibility, Register, RegisterClass, register};
use zydis::ISAExt;

/// Whether `insn` is an x87 instruction.
pub(in crate::codegen::optimizations) fn is_x87(insn: &Insn) -> bool {
    insn.instruction().meta.isa_ext == ISAExt::X87
}

/// Whether `insn` is an MMX instruction: it names an `mm` register, or is `emms`.
pub(in crate::codegen::optimizations) fn is_mmx(insn: &Insn) -> bool {
    matches!(insn.mnemonic(), Mnemonic::EMMS | Mnemonic::FEMMS)
        || visible_registers(insn).any(|reg| reg.class() == RegisterClass::MMX)
}

/// The registers `insn`'s visible operands name.
pub(in crate::codegen::optimizations) fn visible_registers(
    insn: &Insn,
) -> impl Iterator<Item = Register> + '_ {
    let count = usize::from(insn.instruction().operand_count);

    insn.operands()[..count]
        .iter()
        .filter(|operand| operand.visibility != OperandVisibility::HIDDEN)
        .filter_map(register)
}
