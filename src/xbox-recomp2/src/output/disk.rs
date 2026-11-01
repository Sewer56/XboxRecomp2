//! Writes the generated files to disk.

use super::render::{
    render_config_header, render_context_header, render_func_mapping, render_shared_header,
    render_split_include,
};
use super::{OUT_RESERVE, OutError, OutputWriter};
use crate::config::RecompilerConfig;
use crate::executable::Executable;
use std::fs::{canonicalize, create_dir_all, read, symlink_metadata, write};
use std::io;
use std::path::{Path, PathBuf};

/// Saves generated files into the output directory.
pub struct DiskWriter {
    /// Bytes of the current, unsaved file.
    out: Vec<u8>,

    /// Index for the next `x86_recomp.<index>.cpp`.
    next_cpp_index: usize,

    /// Absolute output directory.
    root: PathBuf,
}

impl DiskWriter {
    /// Creates a writer for `config`'s output directory, creating if needed.
    ///
    /// # Arguments
    ///
    /// * `config` - recompiler config
    ///
    /// # Returns
    ///
    /// A writer that saves into the configured absolute path.
    ///
    /// # Errors
    ///
    /// Returns [`OutError::Io`] if the directory can't be created or its
    /// absolute path can't be found.
    pub fn new(config: &RecompilerConfig) -> Result<Self, OutError> {
        // Empty means the current directory.
        let config_dir = if config.directory_path.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(&*config.directory_path)
        };

        // An absolute `out_directory_path` replaces `config_dir`.
        let root = config_dir.join(&*config.out_directory_path);
        create_dir_all(&root).map_err(|source| OutError::Io {
            path: root.clone(),
            source,
        })?;

        let root = canonicalize(&root).map_err(|source| OutError::Io { path: root, source })?;

        Ok(Self {
            out: Vec::with_capacity(OUT_RESERVE),
            next_cpp_index: 0,
            root,
        })
    }

    /// Gets name for the next file to output.
    ///
    /// # Returns
    ///
    /// A name such as `x86_recomp.3.cpp`.
    ///
    /// [`save`]: DiskWriter::save
    pub fn next_unit_name(&self) -> String {
        format!("x86_recomp.{}.cpp", self.next_cpp_index)
    }

    /// Writes the current file to disk and starts a new one.
    ///
    /// Skips empty/unchanged files.
    ///
    /// # Arguments
    ///
    /// * `name` - file name, or [`None`] for the next `x86_recomp.<index>.cpp`.
    ///
    /// # Errors
    ///
    /// - [`OutError::Io`] if reading or writing the file fails.
    /// - [`OutError::OutputPathIsSymlink`] if the path is a symlink.
    pub fn save(&mut self, name: Option<&str>) -> Result<(), OutError> {
        if self.out.is_empty() {
            return Ok(());
        }

        let unit_name;
        let file_name = match name {
            Some(name) => name,
            None => {
                unit_name = self.next_unit_name();
                self.next_cpp_index += 1;
                &unit_name
            }
        };

        let path = self.root.join(file_name);

        let should_write = match read(&path) {
            Ok(existing) => existing != self.out,
            Err(_) => true,
        };

        if should_write {
            if is_symlink(&path)? {
                return Err(OutError::OutputPathIsSymlink { path });
            }

            if let Err(source) = write(&path, &self.out) {
                return Err(OutError::Io { path, source });
            }
        }

        self.out.clear();
        Ok(())
    }

    /// Adds raw bytes to the current file.
    ///
    /// # Arguments
    ///
    /// * `bytes` - usually one function's C++.
    pub fn append_bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    /// Adds `x86_config.h` content.
    ///
    /// # Arguments
    ///
    /// * `executable` - the executable being processed.
    pub fn write_config_header(&mut self, executable: &Executable) {
        render_config_header(executable, &mut self.out);
    }

    /// Adds `x86_context.h` content.
    ///
    /// # Arguments
    ///
    /// * `context` - added as is, after an include line.
    pub fn write_context_header(&mut self, context: &[u8]) {
        render_context_header(context, &mut self.out);
    }

    /// Adds `x86_recomp_shared.h` content.
    ///
    /// # Arguments
    ///
    /// * `executable` - each symbol gets an `extern` line.
    /// * `config` - controls which functions can be replaced with `X86_OVERRIDE_FUNC`.
    pub fn write_shared_header(&mut self, executable: &Executable, config: &RecompilerConfig) {
        render_shared_header(executable, config, &mut self.out);
    }

    /// Adds `x86_func_mapping.cpp` content.
    ///
    /// # Arguments
    ///
    /// * `executable` - each symbol gets an address-to-function entry.
    pub fn write_func_mapping(&mut self, executable: &Executable) {
        render_func_mapping(executable, &mut self.out);
    }

    /// Adds the `#include` line that opens each `x86_recomp.<index>.cpp`.
    pub fn write_split_include(&mut self) {
        render_split_include(&mut self.out);
    }
}

impl OutputWriter for DiskWriter {
    fn write_config_header(&mut self, executable: &Executable) {
        self.write_config_header(executable);
    }

    fn write_context_header(&mut self, context: &[u8]) {
        self.write_context_header(context);
    }

    fn write_shared_header(&mut self, executable: &Executable, config: &RecompilerConfig) {
        self.write_shared_header(executable, config);
    }

    fn write_func_mapping(&mut self, executable: &Executable) {
        self.write_func_mapping(executable);
    }

    fn write_split_include(&mut self) {
        self.write_split_include();
    }

    fn append_bytes(&mut self, bytes: &[u8]) {
        self.append_bytes(bytes);
    }

    fn save(&mut self, name: Option<&str>) -> Result<(), OutError> {
        self.save(name)
    }
}

fn is_symlink(path: &Path) -> Result<bool, OutError> {
    match symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(OutError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}
