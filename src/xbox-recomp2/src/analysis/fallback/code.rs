//! Helpers for walking through code sections one instruction at a time.

use crate::analysis::pointer_tables::PointerScan;
use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder, Insn};
use crate::executable::{Executable, Section, SectionFlags};
use xbox_recomp2_executable::GuestAddress32;

/// Walks each code section and calls `on_instruction` for every instruction.
///
/// # Arguments
///
/// - `executable`: the sections and bytes to walk.
/// - `decoder`: decodes the instructions.
/// - `scan`: for pointer scanning.
/// - `on_instruction`: gets the address, instruction, section and `scan`.
///
/// # Remarks
///
/// Ignores any excluded bytes.
/// If decode fails, it skips ahead one byte and tries again.
pub(super) fn for_each_decoded_instruction(
    executable: &Executable,
    decoder: &Decoder,
    scan: &mut PointerScan<'_>,
    mut on_instruction: impl FnMut(GuestAddress32, &Insn, &Section, &mut PointerScan<'_>),
) {
    let mut buffer = DecodeBuffer::new();

    for section in &executable.sections {
        if !section.flags.contains(SectionFlags::CODE) {
            continue;
        }

        // Scan from the section base to end of file bytes.
        // Remember, some sections are longer in virtual mem than on disk
        // (they get 0 padded!)
        let section_end = file_backed_end(executable, section);
        let mut address = section.base.get();

        while address < section_end {
            // Don't read past the section or into exclusions.
            let window = scan.code_window(GuestAddress32::from(address as u32));
            let window = &window[..window.len().min((section_end - address) as usize)];

            // Bad decode? Try the next byte.
            let DecodeOutcome::Ok(insn) = buffer.decode_at(decoder, window, 0) else {
                address += 1;
                continue;
            };

            // Pass to caller
            on_instruction(GuestAddress32::from(address as u32), insn, section, scan);

            address += insn.length() as u64;
        }
    }
}

/// Finds where the section's bytes in the file end.
///
/// # Arguments
///
/// - `executable`: the file bytes and address limit.
/// - `section`: the section to check.
///
/// # Returns
///
/// The end address (exclusive), no higher than the address limit or
/// [`u32::MAX`] + 1.
pub(super) fn file_backed_end(executable: &Executable, section: &Section) -> u64 {
    // Skip any bytes not in file. (In virtual mem, but not file)
    let file_backed_length = section.file_bytes(&executable.data).len();

    // Don't go past the address limits.
    section
        .base
        .get()
        .saturating_add(file_backed_length as u64)
        .min(1u64 << 32)
        .min(
            executable
                .address_limit
                .map_or(u64::MAX, |limit| limit.get()),
        )
}
