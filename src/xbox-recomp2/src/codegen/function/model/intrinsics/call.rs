//! A guest `call` instruction.

use super::super::GuestOp;

/// A guest `call`, plus how far it moves ESP before entering the callee.
#[derive(Clone, Debug)]
pub(crate) struct Call {
    /// The original instruction.
    pub guest: GuestOp,

    /// Bytes to move ESP down by before entering the function.
    ///
    /// Usually 4, just for return address.
    ///
    /// Optimisations can however change it:
    ///
    /// - Return slot elimination can remove 4.
    /// - Stack argument elimination adds 4 for each argument `push` it removes.
    pub stack_bytes: u32,

    /// Whether to call the callee's `_fastcall` entry.
    ///
    /// Set by optimizations that change the callee's calling convention.
    pub calls_fastcall: bool,
}
