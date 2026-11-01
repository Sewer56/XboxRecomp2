//! Decodes a function's bytes in order, from start to end.

use super::{BlockExit, StepFailure, WalkStep};
use crate::config::RecompilerConfig;
use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder, RangeDecoder};
use xbox_recomp2_executable::GuestAddress32;

/// Decodes a function from `fixes.functions`, from start to end.``
pub(in super::super) struct InstructionWalk<'a> {
    range: RangeDecoder<'a>,
}

impl<'a> InstructionWalk<'a> {
    /// Creates a walk over `size` bytes starting at `base`.
    ///
    /// # Arguments
    ///
    /// * `config` - bytes to skip.
    /// * `decoder` - decodes the instructions.
    /// * `buffer` - reusable decode scratch space.
    /// * `data` - the executable's bytes, starting at `base`.
    /// * `base` - address of the function's first byte.
    /// * `size` - function size in bytes. May be a guess.
    pub(in super::super) fn new(
        config: &'a RecompilerConfig,
        decoder: &'a Decoder,
        buffer: &'a mut DecodeBuffer,
        data: &'a [u8],
        base: GuestAddress32,
        size: u32,
    ) -> Self {
        Self {
            range: RangeDecoder::new(config, decoder, buffer, data, base, size),
        }
    }

    /// Returns the next step, or [`None`] at the end of the function.
    pub(super) fn next_step(&mut self) -> Option<WalkStep<'_>> {
        let step = self.range.next_step()?;
        Some(match step.decoded {
            DecodeOutcome::Ok(insn) => WalkStep {
                addr: step.address,
                is_last_instruction: step.last,
                is_final_step: step.last,
                exit: BlockExit::Continue,
                decoded: Ok(insn),
            },
            DecodeOutcome::Failed { status, .. } => WalkStep {
                addr: step.address,
                is_last_instruction: false,
                is_final_step: false,
                exit: BlockExit::Continue,
                decoded: Err(StepFailure::Decode(status)),
            },
        })
    }
}
