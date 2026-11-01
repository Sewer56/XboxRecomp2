//! Decode guest instructions using a reusable buffer.

use super::{DecodedOperand, Insn, Status};
use core::mem::MaybeUninit;
use core::ptr;
use zydis::ffi;
use zydis::{MAX_OPERAND_COUNT, MachineMode, StackWidth};

/// Reusable storage for one decoded instruction.
///
/// # Usage
///
/// The returned [`Insn`] uses this buffer.
/// Finish with it before decoding again.
pub struct DecodeBuffer {
    insn: MaybeUninit<Insn>,
}

/// The result of one decode attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeOutcome<'insn> {
    /// The decoded instruction.
    Ok(&'insn Insn),

    /// Decoding failed.
    Failed {
        /// Why decoding failed.
        status: Status,

        /// Bytes consumed before the failure; 0 if none.
        partial_length: u8,
    },
}

/// Decodes guest x86 instructions.
#[derive(Clone, Debug)]
pub struct Decoder {
    inner: ffi::Decoder,
}

impl DecodeBuffer {
    /// Creates an empty buffer.
    pub fn new() -> Self {
        // Zeroed memory is a valid unused operand.
        let operands =
            unsafe { Box::<[DecodedOperand]>::new_zeroed_slice(MAX_OPERAND_COUNT).assume_init() };

        let mut insn = MaybeUninit::<Insn>::uninit();
        unsafe { ptr::addr_of_mut!((*insn.as_mut_ptr()).operands).write(operands) };
        Self { insn }
    }

    /// Decodes the first instruction in `buffer`.
    ///
    /// # Arguments
    ///
    /// * `decoder` - the decoder to use.
    /// * `buffer` - guest bytes, starting at the instruction.
    ///
    /// # Returns
    ///
    /// - [`DecodeOutcome::Ok`]: the instruction.
    /// - [`DecodeOutcome::Failed`]: the status and bytes consumed.
    /// - [`DecodeOutcome::Failed`] with [`Status::NoMoreData`]: empty input.
    #[inline]
    pub fn decode_single_instruction(
        &mut self,
        decoder: &Decoder,
        buffer: &[u8],
    ) -> DecodeOutcome<'_> {
        if buffer.is_empty() {
            return DecodeOutcome::Failed {
                status: Status::NoMoreData,
                partial_length: 0,
            };
        }

        let insn_ptr = self.insn.as_mut_ptr();
        let status = unsafe {
            ffi::ZydisDecoderDecodeFull(
                &decoder.inner,
                buffer.as_ptr().cast(),
                buffer.len(),
                ptr::addr_of_mut!((*insn_ptr).instruction),
                (*insn_ptr)
                    .operands
                    .as_mut_ptr()
                    .cast::<[DecodedOperand; MAX_OPERAND_COUNT]>(),
            )
        };

        if !status.is_error() {
            return DecodeOutcome::Ok(unsafe { &*insn_ptr });
        }

        let partial_length = unsafe { ptr::addr_of!((*insn_ptr).instruction.length).read() };
        DecodeOutcome::Failed {
            status,
            partial_length,
        }
    }

    /// Decodes the instruction at `offset` in `data`.
    #[inline]
    pub(crate) fn decode_at(
        &mut self,
        decoder: &Decoder,
        data: &[u8],
        offset: usize,
    ) -> DecodeOutcome<'_> {
        match data.get(offset..) {
            Some(bytes) => self.decode_single_instruction(decoder, bytes),
            None => DecodeOutcome::Failed {
                status: Status::NoMoreData,
                partial_length: 0,
            },
        }
    }
}

impl Decoder {
    /// Creates a [`MachineMode::LEGACY_32`] decoder for walking functions.
    pub fn new() -> Self {
        Self::with_mode(MachineMode::LEGACY_32)
    }

    /// Creates a [`MachineMode::LONG_COMPAT_32`] decoder for finding functions.
    pub fn new_compatibility_mode() -> Self {
        Self::with_mode(MachineMode::LONG_COMPAT_32)
    }

    /// Creates a decoder for `mode` with a 32-bit stack.
    fn with_mode(mode: MachineMode) -> Self {
        let mut raw = MaybeUninit::<ffi::Decoder>::uninit();
        let status = unsafe { ffi::ZydisDecoderInit(raw.as_mut_ptr(), mode, StackWidth::_32) };
        assert_eq!(
            status,
            Status::Success,
            "ZydisDecoderInit rejected {mode:?} with a 32-bit stack"
        );

        Self {
            inner: unsafe { raw.assume_init() },
        }
    }
}

impl Drop for DecodeBuffer {
    fn drop(&mut self) {
        // Free the operands. `MaybeUninit` won't do it for us.
        unsafe { ptr::drop_in_place(ptr::addr_of_mut!((*self.insn.as_mut_ptr()).operands)) };
    }
}

impl Default for DecodeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Mnemonic;

    #[test]
    fn decode_at_should_start_at_requested_offset() {
        // Arrange
        let data = [
            0x90, // 0: nop
            0xB8, 0x44, 0x33, 0x22, 0x11, // 1: mov eax, 0x11223344
        ];
        let mut buffer = DecodeBuffer::new();

        // Act
        let outcome = buffer.decode_at(&Decoder::new(), &data, 1);

        // Assert
        let DecodeOutcome::Ok(insn) = outcome else {
            panic!("expected the MOV at offset 1, got {outcome:?}");
        };
        assert_eq!(insn.mnemonic(), Mnemonic::MOV);
        assert_eq!(insn.length(), 5);
    }
}
