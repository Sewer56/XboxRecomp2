//! A run of bytes on the guest stack.

use crate::codegen::optimizations::helpers::instruction::general_registers::GeneralRegisters;
use crate::decode::{MemoryInfo, Register};

/// A run of bytes on the guest stack.
///
/// ```text
/// mov dword [esp+8], eax  ->  StackRange { base: ESP, start: 8,  end: 12 }
/// mov word  [ebp-4], ax   ->  StackRange { base: EBP, start: -4, end: -2 }
/// ```
///
/// `end` is exclusive, so the first covers `[esp+8]` to `[esp+11]`.
#[derive(Clone, Copy)]
pub(super) struct StackRange {
    /// ESP or EBP.
    base: GeneralRegisters,

    /// Offset of the first byte.
    start: i32,

    /// Offset after the last byte.
    end: i32,
}

impl StackRange {
    /// The bytes that a `bits` wide access to `mem` touches.
    ///
    /// # Returns
    ///
    /// [`None`] for an indexed access like `[esp+eax*4]`.
    pub(super) fn of(mem: &MemoryInfo, bits: u16) -> Option<Self> {
        if mem.index != Register::NONE {
            return None;
        }

        let start = i32::try_from(mem.disp.displacement).ok()?;
        let end = start.checked_add(i32::from(bits / 8))?;
        Some(Self {
            base: GeneralRegisters::of(mem.base),
            start,
            end,
        })
    }

    /// Whether the two ranges share a byte.
    pub(super) fn overlaps(self, other: Self) -> bool {
        // Offsets from ESP and EBP can't be compared.
        if self.base != other.base {
            return false;
        }

        self.start < other.end && other.start < self.end
    }
}
