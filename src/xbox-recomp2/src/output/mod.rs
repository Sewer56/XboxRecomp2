//! Writes the generated C++ files to disk or memory.
//!
//! - [`DiskWriter`] saves them to the output directory.
//! - [`MemoryWriter`] saves them to memory.

use crate::config::RecompilerConfig;
use crate::executable::Executable;
pub use disk::DiskWriter;
pub use memory_writer::GeneratedFileSet;
pub(crate) use memory_writer::MemoryWriter;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

mod disk;
mod memory_writer;
mod render;

/// Starting capacity of the output buffer: 10 MiB.
pub(crate) const OUT_RESERVE: usize = 10 * 1024 * 1024;

/// Errors from writing the generated files.
#[derive(Debug, Error)]
pub enum OutError {
    /// An output path is a symlink.
    /// (You should probably remove it and run again.)
    #[error("{path}: output path is a symlink")]
    OutputPathIsSymlink {
        /// The symlink.
        path: PathBuf,
    },

    /// Reading, writing or creating a path failed.
    #[error("{path}: {source}")]
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
}

/// Receives the generated files, one at a time.
///
/// # Usage
///
/// 1. Add content with the `write_*` methods or [`append_bytes`].
/// 2. Call [`save`] to output a single file.
///
/// [`append_bytes`]: OutputWriter::append_bytes
/// [`save`]: OutputWriter::save
pub(crate) trait OutputWriter {
    /// Adds `x86_config.h` content.
    fn write_config_header(&mut self, executable: &Executable);

    /// Adds `x86_context.h` content: an include line, then `context`.
    fn write_context_header(&mut self, context: &[u8]);

    /// Adds `x86_recomp_shared.h` content.
    fn write_shared_header(&mut self, executable: &Executable, config: &RecompilerConfig);

    /// Adds `x86_func_mapping.cpp` content.
    fn write_func_mapping(&mut self, executable: &Executable);

    /// Adds the `#include` line that opens each `x86_recomp.<index>.cpp`.
    fn write_split_include(&mut self);

    /// Adds raw bytes, usually one function's C++.
    fn append_bytes(&mut self, bytes: &[u8]);

    /// Saves the current file as `name`, or as the next
    /// `x86_recomp.<index>.cpp` if [`None`].
    ///
    /// # Errors
    ///
    /// Returns [`OutError`] if saving fails.
    fn save(&mut self, name: Option<&str>) -> Result<(), OutError>;
}
