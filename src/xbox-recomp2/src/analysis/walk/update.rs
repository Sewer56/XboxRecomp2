//! Report what changed after walking a function.

use xbox_recomp2_executable::GuestAddress32;

/// Results the work queue needs from a walk.
#[derive(Debug, Default)]
pub(in crate::analysis) struct WalkUpdate {
    /// Whether the function node or a referenced function changed.
    /// Basically, whether we added any new previously undiscovered functions.
    pub changed: bool,

    /// New function entries to visit, sorted by address.
    pub new_functions: Vec<GuestAddress32>,
}
