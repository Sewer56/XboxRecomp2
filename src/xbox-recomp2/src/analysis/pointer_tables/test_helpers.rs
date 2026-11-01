//! Common test helpers.

use super::{Decoder, PointerScan, RecompilerConfig, Resolution};
use crate::analysis::graph::FunctionNode;
use crate::analysis::graph::test_helpers;
use crate::executable::{Executable, Section, SectionFlags};
use xbox_recomp2_executable::GuestAddress32;

/// Aligned callback prologue in `.text`.
pub(in crate::analysis) const CALLBACK: u32 = 0x1080;
/// Start of the `.data` pointer table.
pub(in crate::analysis) const TABLE: u32 = 0x2000;
/// Unaligned `ret` in `.text`, with no prologue.
pub(in crate::analysis) const UNALIGNED: u32 = 0x1091;

/// [`CALLBACK`] as a guest address.
pub(in crate::analysis) fn callback() -> GuestAddress32 {
    GuestAddress32::from(CALLBACK)
}

/// Builds a function from `code` and runs [`resolve`] on it.
pub(in crate::analysis) fn resolution(
    code: &[u8],
    executable: &Executable,
    config: &RecompilerConfig,
) -> Resolution {
    let function = test_helpers::function(code);

    resolve(executable, config, &function)
}

/// [`UNALIGNED`] as a guest address.
pub(in crate::analysis) fn unaligned() -> GuestAddress32 {
    GuestAddress32::from(UNALIGNED)
}

/// Scans `function` and returns its sorted, unique references.
pub(in crate::analysis) fn resolve(
    executable: &Executable,
    config: &RecompilerConfig,
    function: &FunctionNode,
) -> Resolution {
    let decoder = Decoder::new_compatibility_mode();
    let mut scan = PointerScan::new(executable, config, &decoder);
    let mut result = Resolution::default();
    for instruction in function.instructions.values() {
        scan.instruction(instruction, &mut result);
    }
    result.references.sort_unstable();
    result.references.dedup();

    result
}

/// Maps `code` plus a callback prologue and a `.data` table of `entries`.
pub(in crate::analysis) fn executable(code: &[u8], entries: &[u32]) -> Executable {
    // - `.text` at 0x1000 (file offset 0).
    // - `.data` at 0x2000 (file offset 0x100).
    // - Unused code bytes are 0xCC (int3).
    //
    // ```text
    // .text:
    //   0x1000: ..              test's `code`
    //   0x1080: 55              push ebp     ; CALLBACK
    //   0x1081: 89 E5           mov ebp, esp
    //   0x1083: C3              ret
    //   0x1091: C3              ret          ; UNALIGNED, no prologue
    // .data (the common two-slot form):
    //   0x2000: 80 10 00 00     dd 0x1080
    //   0x2004: 91 10 00 00     dd 0x1091
    // ```
    let mut text = vec![0xcc; 0x100];
    text[..code.len()].copy_from_slice(code);
    text[0x80..0x84].copy_from_slice(&[0x55, 0x89, 0xe5, 0xc3]);
    text[0x91] = 0xc3;
    let mut executable = test_helpers::executable(&text);
    executable.sections.push(Section::new(
        ".data",
        u64::from(TABLE),
        entries.len() as u32 * 4,
        SectionFlags::DATA,
        text.len() as u32,
    ));
    executable
        .data
        .extend(entries.iter().flat_map(|entry| entry.to_le_bytes()));
    executable.size = executable.data.len() as u32;

    executable
}
