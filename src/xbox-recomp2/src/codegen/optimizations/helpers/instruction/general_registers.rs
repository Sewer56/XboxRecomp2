//! A set of 32-bit general-purpose registers.

use crate::decode::{MachineMode, MemoryInfo, Register};
use bitflags::bitflags;

bitflags! {
    /// A set of 32-bit general-purpose registers, `eax` to `edi`.
    #[derive(Clone, Copy, Default, PartialEq, Eq)]
    pub(in crate::codegen) struct GeneralRegisters: u8 {
        const EAX = 1;
        const ECX = 1 << 1;
        const EDX = 1 << 2;
        const EBX = 1 << 3;
        const ESP = 1 << 4;
        const EBP = 1 << 5;
        const ESI = 1 << 6;
        const EDI = 1 << 7;
    }
}

impl GeneralRegisters {
    /// The 32-bit register that holds `reg`, like `EAX` for `al`.
    ///
    /// # Returns
    ///
    /// An empty set for anything else, like `Register::NONE` or `xmm0`.
    pub(in crate::codegen) fn of(reg: Register) -> Self {
        match reg.largest_enclosing(MachineMode::LEGACY_32) {
            Register::EAX => Self::EAX,
            Register::ECX => Self::ECX,
            Register::EDX => Self::EDX,
            Register::EBX => Self::EBX,
            Register::ESP => Self::ESP,
            Register::EBP => Self::EBP,
            Register::ESI => Self::ESI,
            Register::EDI => Self::EDI,
            _ => Self::empty(),
        }
    }

    /// The registers that `mem`'s address is built from, like `ebx` and `ecx`
    /// for `[ebx+ecx*4+8]`.
    pub(in crate::codegen) fn in_address(mem: &MemoryInfo) -> Self {
        Self::of(mem.base) | Self::of(mem.index)
    }
}
