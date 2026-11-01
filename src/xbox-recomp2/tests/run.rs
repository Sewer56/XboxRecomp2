//! Tests for [`run_with`], the public entry point.

use platform::PLATFORM;
use std::fs;
use std::path::PathBuf;
use synthetic::Synthetic;
use temp_dir::temp_dir;
use xbox_recomp2::RunError;

#[path = "common/platform.rs"]
mod platform;
#[path = "common/synthetic.rs"]
mod synthetic;
#[path = "common/temp_dir.rs"]
mod temp_dir;

// ── Core behaviour ───────────────────────────────────────────────────────────

/// If no function starts at the entry point, the run fails and writes no
/// output.
#[test]
fn run_should_fail_without_entry_function() {
    // Arrange.
    //   0x11000: 90  nop  ; entry point, marked invalid
    //   0x11001: C3  ret
    //   0x11002: C3  ret  ; the only declared function (1 byte)
    let code = [0x90, 0xC3, 0xC3];

    // Act.
    let (dir, result) = run_skipping_entry("missing-entry", &code, 0x11002, 1);

    // Assert.
    assert!(matches!(
        result,
        Err(RunError::MissingEntryPoint { address, .. })
            if address.get() == 0x11000
    ));
    assert!(!dir.join("x86").exists());
    fs::remove_dir_all(dir).unwrap();
}

/// The run skips an invalid entry point byte but still emits `_xstart`.
#[test]
fn run_should_keep_entry_with_skipped_first_byte() {
    // Arrange.
    //   0x11000: 90  nop  ; entry point, marked invalid
    //   0x11001: C3  ret
    // The declared function covers both bytes.
    let code = [0x90, 0xC3];

    // Act.
    let (dir, result) = run_skipping_entry("skipped-entry", &code, 0x11000, 2);

    // Assert.
    result.expect("run");
    let mapping = fs::read_to_string(dir.join("x86/x86_func_mapping.cpp")).unwrap();
    let body = fs::read_to_string(dir.join("x86/x86_recomp.0.cpp")).unwrap();
    assert!(mapping.contains("{ 0x11000, _xstart }"));
    assert!(body.contains("// ret"));
    assert!(!body.contains("// nop"));
    fs::remove_dir_all(dir).unwrap();
}

/// Runs `code` with one declared function at `address` that is `size` bytes
/// long.
fn run_skipping_entry(
    name: &str,
    code: &[u8],
    address: u32,
    size: u32,
) -> (PathBuf, Result<(), RunError>) {
    let dir = temp_dir(name);
    let fixes = format!(
        "functions = [{{ address = {address:#x}, size = {size} }}]\n\
         invalid_instructions = [{{ data = 0x11000, size = 1 }}]"
    );
    let result = Synthetic::new(code).run(&dir, &fixes);
    (dir, result)
}
