//! Builds a fake executable from raw bytes, so tests don't need a real XBE.

// Not every test crate uses every helper.
#![allow(dead_code)]

use super::PLATFORM;
use core::cell::RefCell;
use std::fs;
use std::path::Path;
use xbox_recomp2::config::RecompilerConfig;
use xbox_recomp2::executable::{Executable, ExecutableFormat, Section, SectionFlags};
use xbox_recomp2::{Recompiler, RunError, run_with};

struct FixtureFormat(RefCell<Option<Executable>>);

/// An in-memory executable built from the sections you add.
pub(crate) struct Synthetic(Executable);

impl Synthetic {
    /// Puts `text` in `.text` at 0x11000, which is also the entry point.
    ///
    /// The image starts at 0x10000.
    pub(crate) fn new(text: &[u8]) -> Self {
        Self(Executable {
            base: 0x10000u64.into(),
            entry_point: 0x11000u64.into(),
            ..Executable::default()
        })
        .section(".text", 0x11000, SectionFlags::CODE, text)
    }

    /// Adds a section at address `base`.
    ///
    /// Its bytes go at file offset `base - 0x10000`.
    pub(crate) fn section(
        mut self,
        name: &str,
        base: u64,
        flags: SectionFlags,
        bytes: &[u8],
    ) -> Self {
        // e.g. address 0x11400 lands at file offset 0x1400.
        let offset = (base - u64::from(self.0.base)) as usize;
        self.0.data.resize(offset + bytes.len(), 0);
        self.0.data[offset..].copy_from_slice(bytes);
        self.0.sections.push(Section::new(
            name,
            base,
            bytes.len() as u32,
            flags,
            offset as u32,
        ));
        self.0.size = self.0.data.len() as u32;
        self.0.size_of_image = self.0.data.len() as u64;
        self
    }

    /// Loads this executable into a [`Recompiler`], the same way it loads
    /// a real file.
    pub(crate) fn recompiler(self, config: RecompilerConfig) -> Recompiler<'static> {
        let format = FixtureFormat(RefCell::new(Some(self.0)));
        Recompiler::from_input(config, b"synthetic", &[&format], &PLATFORM)
            .expect("load synthetic executable")
    }

    /// Writes a config to `dir` and passes it to [`run_with`].
    pub(crate) fn run(self, dir: &Path, fixes: &str) -> Result<(), RunError> {
        let config = dir.join("recomp.toml");
        fs::write(dir.join("game.bin"), b"synthetic").expect("write input");
        let toml = format!(
            "[main]\nfile_path = 'game.bin'\nout_directory_path = 'x86'\n[fixes]\n{fixes}\n"
        );
        fs::write(&config, toml).expect("write config");

        let format = FixtureFormat(RefCell::new(Some(self.0)));
        run_with(&config, &[&format], &PLATFORM, &mut Vec::new())
    }
}

impl ExecutableFormat for FixtureFormat {
    fn name(&self) -> &'static str {
        "synthetic"
    }

    fn detect(&self, data: &[u8]) -> bool {
        data == b"synthetic"
    }

    fn load(&self, _data: &[u8]) -> Executable {
        self.0.borrow_mut().take().expect("fixture loaded once")
    }
}
