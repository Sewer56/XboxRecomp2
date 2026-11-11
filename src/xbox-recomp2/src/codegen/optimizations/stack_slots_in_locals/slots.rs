//! One instruction's stack slots.

use super::EspOffset;
use crate::codegen::function::model::FrameAccess;

/// Index of an operand in an instruction.
/// e.g. in `mov [esp+4], eax`, `[esp+4]` is 0 and `eax` is 1.
pub(super) type OperandIndex = u8;

/// One instruction's slots, as (negative) offsets from the entry ESP.
#[derive(Default)]
pub(super) struct Slots {
    pub(super) operands: Vec<(OperandIndex, EspOffset)>,

    /// Offset of a `push`, `pop` or `leave`.
    pub(super) push_pop: Option<EspOffset>,
}

impl Slots {
    /// e.g. `mov eax, [esp+4]` only reads an argument, so it's empty.
    pub(super) fn is_empty(&self) -> bool {
        self.operands.is_empty() && self.push_pop.is_none()
    }

    /// Lowest offset, or 0 if empty.
    ///
    /// e.g. `push [esp]` right after `push esi`:
    ///
    /// ```text
    /// offset  slot
    ///   -4    saved esi  <- read
    ///   -8    copy       <- written: lowest
    /// ```
    pub(super) fn lowest(&self) -> EspOffset {
        let operands = self.operands.iter().map(|&(_, offset)| offset);
        operands.chain(self.push_pop).min().unwrap_or(0)
    }

    /// Converts to `frame` offsets.
    ///
    /// e.g. with a `size` of 0x10:
    ///
    /// ```text
    /// offset  frame
    ///   -4    frame[0xC]
    ///   -8    frame[0x8]
    ///  -16    frame[0x0]
    /// ```
    pub(super) fn into_frame_access(self, size: i32) -> FrameAccess {
        // `frame[0]` is `size` bytes below the entry ESP.
        let to_frame = |offset: EspOffset| (offset + size) as u32;
        let operands = self.operands.into_iter();
        let operands = operands
            .map(|(index, offset)| (index, to_frame(offset)))
            .collect();

        FrameAccess {
            operands,
            push_pop: self.push_pop.map(to_frame),
        }
    }
}
