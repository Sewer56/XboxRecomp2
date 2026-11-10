//! Which MMX registers an instruction reads and writes.
//!
//! See `mmx-locals.md` for more info.

use super::Effect;
use crate::decode::{Insn, Mnemonic, OperandAction, Register, RegisterClass, register};
use bitflags::bitflags;

bitflags! {
    /// A set of MMX registers, `mm0` to `mm7`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::codegen) struct MmxRegisters: u8 {
        const MM0 = 1;
        const MM1 = 1 << 1;
        const MM2 = 1 << 2;
        const MM3 = 1 << 3;
        const MM4 = 1 << 4;
        const MM5 = 1 << 5;
        const MM6 = 1 << 6;
        const MM7 = 1 << 7;
    }
}

impl MmxRegisters {
    /// The set holding just `reg`.
    ///
    /// # Returns
    ///
    /// [`None`] if `reg` isn't an MMX register.
    pub(in crate::codegen) fn from_register(reg: Register) -> Option<Self> {
        if reg.class() != RegisterClass::MMX {
            return None;
        }

        let index = reg as u16 - Register::MM0 as u16;
        Some(Self::from_bits_retain(1 << index))
    }
}

/// The MMX registers an instruction reads and writes.
pub(in crate::codegen) type MmxEffect = Effect<MmxRegisters>;

impl MmxEffect {
    /// What `insn` does to the `mm` registers.
    pub(in crate::codegen) fn of(insn: &Insn) -> Self {
        // `emms` wipes MMX state
        // https://www.felixcloutier.com/x86/emms ; so we treat it as write of all.
        if matches!(insn.mnemonic(), Mnemonic::EMMS | Mnemonic::FEMMS) {
            return Self {
                reads: MmxRegisters::empty(),
                writes: MmxRegisters::all(),
            };
        }

        let count = usize::from(insn.instruction().operand_count);
        let mut reads = MmxRegisters::empty();
        let mut writes = MmxRegisters::empty();
        for operand in &insn.operands()[..count] {
            // Only `mm` registers matter.
            let reg = match register(operand).and_then(MmxRegisters::from_register) {
                Some(reg) => reg,
                None => continue,
            };

            let action = operand.action;
            let may_read = action.intersects(OperandAction::READ | OperandAction::CONDREAD);
            let may_write = action.intersects(OperandAction::WRITE | OperandAction::CONDWRITE);
            let always_writes = action.contains(OperandAction::WRITE)
                && !action.intersects(OperandAction::CONDWRITE);

            if may_read || !always_writes {
                reads |= reg;
            }

            if may_write {
                writes |= reg;
            }
        }

        Self { reads, writes }
    }
}
