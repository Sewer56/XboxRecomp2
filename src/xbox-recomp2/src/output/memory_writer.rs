//! Writes the generated files to memory and returns them as a
//! [`GeneratedFileSet`].

use super::render::{
    render_config_header, render_context_header, render_func_mapping, render_shared_header,
    render_split_include,
};
use super::{OUT_RESERVE, OutputWriter};
use crate::OutError;
use crate::config::RecompilerConfig;
use crate::executable::Executable;
use core::mem::replace;

/// All files from single recompiler run.
#[derive(Debug, Default)]
pub struct GeneratedFileSet {
    /// `(name, bytes)` pairs.
    files: Vec<(String, Vec<u8>)>,
}

/// Saves each generated file to memory.
pub(crate) struct MemoryWriter {
    /// Bytes of the current, unsaved file.
    out: Vec<u8>,

    /// Index for the next `x86_recomp.<index>.cpp`.
    next_cpp_index: usize,

    /// Saved `(name, bytes)` pairs.
    files: Vec<(String, Vec<u8>)>,
}

impl GeneratedFileSet {
    /// Iterates over the files.
    ///
    /// # Returns
    ///
    /// A `(name, bytes)` pair per file.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
    }

    /// Finds a file by name.
    ///
    /// # Arguments
    ///
    /// * `name` - such as `x86_config.h` or `x86_recomp.0.cpp`.
    ///
    /// # Returns
    ///
    /// The file's bytes, or [`None`].
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(file, _)| file == name)
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// Adds up the size of every file.
    ///
    /// # Returns
    ///
    /// The total size in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|(_, bytes)| bytes.len() as u64).sum()
    }
}

impl MemoryWriter {
    /// Creates an empty writer.
    pub(crate) fn new() -> Self {
        Self {
            out: Vec::with_capacity(OUT_RESERVE),
            files: Vec::new(),
            next_cpp_index: 0,
        }
    }

    /// Returns the saved files, dropping any unsaved bytes.
    pub(crate) fn finish(self) -> GeneratedFileSet {
        GeneratedFileSet { files: self.files }
    }
}

impl OutputWriter for MemoryWriter {
    fn write_config_header(&mut self, executable: &Executable) {
        render_config_header(executable, &mut self.out);
    }

    fn write_context_header(&mut self, context: &[u8]) {
        render_context_header(context, &mut self.out);
    }

    fn write_shared_header(&mut self, executable: &Executable, config: &RecompilerConfig) {
        render_shared_header(executable, config, &mut self.out);
    }

    fn write_func_mapping(&mut self, executable: &Executable) {
        render_func_mapping(executable, &mut self.out);
    }

    fn write_split_include(&mut self) {
        render_split_include(&mut self.out);
    }

    fn append_bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    fn save(&mut self, name: Option<&str>) -> Result<(), OutError> {
        if self.out.is_empty() {
            return Ok(());
        }

        let unit_name;
        let file_name = match name {
            Some(name) => name,
            None => {
                unit_name = format!("x86_recomp.{}.cpp", self.next_cpp_index);
                self.next_cpp_index += 1;
                &unit_name
            }
        };

        // Start the next buffer at this file's size.
        let flushed = self.out.len();
        let bytes = replace(&mut self.out, Vec::with_capacity(flushed));
        self.files.push((file_name.to_owned(), bytes));

        Ok(())
    }
}
