//! Format decoded instructions as Intel-style disassembly text.

use super::{Insn, Status};
use core::mem::MaybeUninit;
use core::ptr::null_mut;
use core::str::from_utf8;
use zydis::FormatterStyle;
use zydis::ffi;

/// Buffer size of single instruction.
pub const DISASM_BUFFER_SIZE: usize = 256;

/// Formats decoded instructions as text.
pub struct DisasmFormatter {
    inner: ffi::Formatter,
}

impl DisasmFormatter {
    /// Creates a formatter using the default Intel style.
    pub fn new() -> Self {
        let mut inner = MaybeUninit::<ffi::Formatter>::uninit();

        let status = unsafe { ffi::ZydisFormatterInit(inner.as_mut_ptr(), FormatterStyle::INTEL) };
        assert_eq!(
            status,
            Status::Success,
            "ZydisFormatterInit rejected the Intel style"
        );

        Self {
            inner: unsafe { inner.assume_init() },
        }
    }

    /// Formats `insn` as disassembly text.
    ///
    /// # Arguments
    ///
    /// * `insn` - the instruction to format.
    /// * `base` - runtime address, for branch targets.
    /// * `buffer` - receives NUL-terminated text. Use [`DISASM_BUFFER_SIZE`].
    ///
    /// # Returns
    ///
    /// The text.
    ///
    /// # Errors
    ///
    /// - The Zydis status, e.g. if `buffer` is too small.
    /// - [`Status::FormatterError`] if text isn't UTF-8.
    pub fn format<'buf>(
        &self,
        insn: &Insn,
        base: u64,
        buffer: &'buf mut [u8],
    ) -> Result<&'buf str, Status> {
        let status = unsafe {
            ffi::ZydisFormatterFormatInstruction(
                &self.inner,
                &insn.instruction,
                insn.operands.as_ptr(),
                insn.instruction.operand_count,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                base,
                null_mut(),
            )
        };

        if status.is_error() {
            return Err(status);
        }

        let len = buffer
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(buffer.len());
        from_utf8(&buffer[..len]).map_err(|_| Status::FormatterError)
    }
}

impl Default for DisasmFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder};
    use rstest::rstest;

    #[rstest]
    #[case::intel_operands(&[0x89, 0xD8], 0x1000, "mov eax, ebx")]
    #[case::branch_target(&[0x74, 0x0A], 0x2000, "jz 0x0000200C")]
    #[case::relocated_branch_target(&[0x74, 0x0A], 0x3000, "jz 0x0000300C")]
    fn format_should_render_intel_text_at_runtime_address(
        #[case] bytes: &[u8],
        #[case] base: u64,
        #[case] expected: &str,
    ) {
        // Arrange
        let mut buffer = DecodeBuffer::new();
        let DecodeOutcome::Ok(insn) = buffer.decode_single_instruction(&Decoder::new(), bytes)
        else {
            panic!("sample should decode");
        };
        let mut text = [0xA5; DISASM_BUFFER_SIZE];

        // Act
        let rendered = DisasmFormatter::new().format(insn, base, &mut text);

        // Assert
        assert_eq!(rendered, Ok(expected));
    }
}
