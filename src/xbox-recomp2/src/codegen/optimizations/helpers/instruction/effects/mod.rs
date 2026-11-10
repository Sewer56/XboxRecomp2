//! What an instruction reads and overwrites, for each kind of register.
//!
//! - [`eflags`]: arithmetic flags, as [`EFlagEffects`](eflags::EFlagEffects).
//! - [`mmx`]: `mm` registers, as [`MmxEffect`](mmx::MmxEffect).
//! - [`x87_status`]: x87 condition codes, as
//!   [`StatusEffect`](x87_status::StatusEffect).

use bitflags::Flags;

pub(in crate::codegen) mod eflags;
pub(in crate::codegen) mod mmx;
pub(in crate::codegen) mod x87_status;

/// What an instruction, or several, read and overwrite.
#[derive(Clone, Copy, Debug)]
pub(in crate::codegen) struct Effect<F> {
    /// What it needs from earlier code.
    pub reads: F,

    /// What it always overwrites, so earlier values no longer matter.
    pub writes: F,
}

impl<F: Flags + Copy> Effect<F> {
    /// Reads and writes nothing.
    pub(in crate::codegen) fn none() -> Self {
        Self {
            reads: F::empty(),
            writes: F::empty(),
        }
    }

    /// Combines two instructions that run in order, `self` then `next`, into one.
    pub(in crate::codegen) fn then(self, next: Self) -> Self {
        Self {
            reads: self.reads.union(next.reads.difference(self.writes)),
            writes: self.writes.union(next.writes),
        }
    }

    /// What earlier code must keep, given what later code needs.
    pub(in crate::codegen) fn needed_before(self, needed_after: F) -> F {
        needed_after.difference(self.writes).union(self.reads)
    }
}
