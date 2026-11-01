//! Mid-assembly hooks, as optimisations see them.

use xbox_recomp2_executable::GuestAddress32;

/// A mid-assembly hook: host code that runs before a guest instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hook {
    /// The instruction the hook runs before.
    pub address: GuestAddress32,
}
