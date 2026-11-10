//! Mid-assembly hooks, as optimisations see them.

use xbox_recomp2_executable::GuestAddress32;

/// A mid-assembly hook: host code that runs before a guest instruction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hook {
    /// The instruction the hook runs before.
    pub address: GuestAddress32,

    /// Whether the hook may return from the function instead.
    pub may_return: bool,

    /// Guest addresses the hook may jump to instead, like `goto loc_1010;`.
    pub jumps: Vec<GuestAddress32>,
}
