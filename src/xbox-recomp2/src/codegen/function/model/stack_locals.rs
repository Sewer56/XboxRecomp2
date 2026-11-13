//! Stack slots moved into a local `frame` array by `stack_slots_in_locals`.

use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// A function's local `frame` array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StackLocals {
    /// Multiple of 16.
    pub size: u32,

    /// Frame accesses, by instruction address.
    pub instructions: BTreeMap<GuestAddress32, FrameAccess>,
}

/// One instruction's frame accesses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameAccess {
    /// `(operand index, frame offset)` pairs.
    pub operands: Vec<(u8, u32)>,

    /// Frame offset of a `push` or `pop`.
    pub push_pop: Option<u32>,
}
