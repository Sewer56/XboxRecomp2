//! Where one stack access goes.

use super::EspOffset;

/// Where one stack access goes.
pub(super) enum SlotUse {
    /// At or above the entry ESP. Stays in guest memory.
    Outside,

    /// Always negative.
    Local(EspOffset),

    /// Below the entry ESP, but can't be rewritten.
    Unsupported,
}
