//! Tests for writing recompiled output to disk.

use common::{Synthetic, smoke_text, temp_dir};
use std::fs;
use std::path::Path;
use xbox_recomp2::Recompiler;
use xbox_recomp2::config::{FunctionSize, RecompilerConfig};
use xbox_recomp2::executable::SectionFlags;

mod common;

/// Output can go above the config directory, like `../outside/nested`.
/// The recompiler creates any missing folders.
#[test]
fn output_directory_should_create_parents_when_above_config_directory() {
    // Arrange.
    let root = temp_dir("traversal");
    let dir = root.join("config");
    fs::create_dir(&dir).unwrap();
    let mut recompiler = fixture(&dir, "../outside/nested");

    // Act.
    recompiler.recompile(&mut Vec::new()).expect("recompile");

    // Assert.
    assert_output_at(&root.join("outside/nested"));
    fs::remove_dir_all(root).unwrap();
}

/// A rerun restores output files that someone changed by hand.
#[test]
fn rerun_should_rewrite_changed_output_file() {
    // Arrange.
    let dir = temp_dir("rewrite");
    fixture(&dir, "x86").recompile(&mut Vec::new()).unwrap();
    let path = dir.join("x86/x86_config.h");
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"tampered\n").unwrap();

    // Act.
    fixture(&dir, "x86").recompile(&mut Vec::new()).unwrap();

    // Assert.
    assert_eq!(fs::read(path).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

fn assert_output_at(dir: &Path) {
    assert!(dir.join("x86_config.h").exists());
    assert!(dir.join("x86_recomp.0.cpp").exists());
}

/// Builds a fake recompiler that writes to `out_dir`, relative to `dir`.
fn fixture(dir: &Path, out_dir: &str) -> Recompiler<'static> {
    let mut config = RecompilerConfig {
        directory_path: dir.to_string_lossy().into_owned().into(),
        out_directory_path: out_dir.into(),
        ..RecompilerConfig::default()
    };
    for (address, size) in [(0x110F0, 2), (0x11020, 3), (0x11030, 5)] {
        config
            .functions
            .insert_or_overwrite(address.into(), FunctionSize(size));
    }

    let mut recompiler = Synthetic::new(&smoke_text())
        .section(".data", 0x11400, SectionFlags::DATA, &[0; 0x100])
        .recompiler(config);
    recompiler.analyse(&mut Vec::new());
    recompiler.rename_entry_symbol();
    recompiler
}
