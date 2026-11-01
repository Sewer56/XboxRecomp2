//! A guest `ret` instruction.

use super::super::GuestOp;

/// A guest `ret`, plus how much stack it should release.
#[derive(Clone, Debug)]
pub(crate) struct Return {
    /// The original instruction, e.g. `ret 8`.
    pub guest: GuestOp,

    /// Bytes of arguments to remove from the stack, e.g. 8 for `ret 8`.
    pub argument_bytes: u16,

    /// If set, there's no return address on the stack to remove.
    pub omit_return_slot: bool,
}
