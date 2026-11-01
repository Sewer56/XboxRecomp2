//! Decodes every instruction in a function from `fixes.functions`.

use super::{DecodeBuffer, DecodeOutcome, Decoder};
use crate::config::{FunctionAddress, RecompilerConfig};
use xbox_recomp2_executable::GuestAddress32;

/// Decodes every instruction from `base` to `base + size`, in order.
///
/// Moves by the instruction's length, or by 1 byte if decoding fails.
/// Skips bytes the config marks:
///
/// - Banned addresses: 1 byte at a time.
/// - Invalid instructions: the whole range at once.
pub(crate) struct RangeDecoder<'a> {
    config: &'a RecompilerConfig,
    decoder: &'a Decoder,
    buffer: &'a mut DecodeBuffer,
    data: &'a [u8],
    base: GuestAddress32,
    size: u64,
    offset: u64,
}

/// One instruction from a [`RangeDecoder`].
pub(crate) struct RangeStep<'a> {
    /// Address of the instruction.
    pub address: GuestAddress32,

    /// Whether this is the last instruction in the range.
    pub last: bool,

    /// The instruction, or why it failed to decode.
    pub decoded: DecodeOutcome<'a>,
}

impl<'a> RangeDecoder<'a> {
    /// Creates a decoder for `size` bytes starting at `base`.
    ///
    /// # Arguments
    ///
    /// - `config`: bytes to skip.
    /// - `decoder`: decodes the instructions.
    /// - `buffer`: reusable decode scratch space.
    /// - `data`: the executable's bytes, starting at `base`.
    /// - `base`: address of the first byte.
    /// - `size`: size in bytes. May be a guess.
    pub(crate) fn new(
        config: &'a RecompilerConfig,
        decoder: &'a Decoder,
        buffer: &'a mut DecodeBuffer,
        data: &'a [u8],
        base: GuestAddress32,
        size: u32,
    ) -> Self {
        Self {
            config,
            decoder,
            buffer,
            data,
            base,
            size: u64::from(size),
            offset: 0,
        }
    }

    /// Returns the next instruction, or [`None`] at the end of the range.
    pub(crate) fn next_step(&mut self) -> Option<RangeStep<'_>> {
        while self.offset < self.size {
            let address = self.base.wrapping_add(self.offset as u32);

            // Skip banned bytes, and invalid instructions.
            if self.config.is_address_range_banned(address.get()) {
                self.offset += 1;
                continue;
            }

            if let Some(skip) = self
                .config
                .invalid_instructions
                .get(FunctionAddress::from(address.get()))
                && skip.0 != 0
            {
                self.offset += u64::from(skip.0);
                continue;
            }

            // Decode this shiz
            let decoded = self
                .buffer
                .decode_at(self.decoder, self.data, self.offset as usize);

            self.offset += match decoded {
                DecodeOutcome::Ok(insn) => insn.length() as u64,
                DecodeOutcome::Failed { .. } => 1,
            };

            return Some(RangeStep {
                address,
                last: self.offset >= self.size,
                decoded,
            });
        }

        None
    }
}
