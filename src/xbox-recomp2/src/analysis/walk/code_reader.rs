//! Give the decoder only mapped code bytes that are not excluded.

use crate::analysis::graph::StopReason;
use crate::analysis::reader::ImageReader;
use crate::config::RecompilerConfig;
use crate::executable::Executable;
use xbox_recomp2_executable::GuestAddress32;

/// Reads code bytes that are safe to decode.
pub(super) struct CodeReader<'a> {
    reader: ImageReader<'a>,
}

impl<'a> CodeReader<'a> {
    /// Builds a code reader using the configured exclusions.
    ///
    /// # Arguments
    ///
    /// - `executable`: mapped sections and stored bytes.
    /// - `config`: banned and invalid-instruction ranges.
    pub fn new(executable: &'a Executable, config: &RecompilerConfig) -> Self {
        Self {
            reader: ImageReader::new(executable, config),
        }
    }

    /// Skips the exclusion containing `address`.
    ///
    /// # Arguments
    ///
    /// - `address`: guest address where execution would continue.
    ///
    /// # Returns
    ///
    /// The first address after the exclusion, or `address` if it is not excluded.
    ///
    /// # Errors
    ///
    /// Same as [`ImageReader::skip_excluded`].
    pub fn skip_excluded(&self, address: GuestAddress32) -> Result<GuestAddress32, StopReason> {
        self.reader.skip_excluded(address)
    }

    /// Reads code bytes so we can decode an instruction at `address`.
    ///
    /// # Arguments
    ///
    /// - `address`: guest address where the instruction would start.
    ///
    /// # Returns
    ///
    /// Up to 15 bytes, enough for the longest x86 instruction. frfr, true
    /// story!
    ///
    /// # Errors
    ///
    /// Same as [`ImageReader::code_window`].
    pub fn window(&self, address: GuestAddress32) -> Result<&'a [u8], StopReason> {
        self.reader.code_window(address)
    }
}
