//! Finds possible table jumps and addresses that may be code.

use super::candidate::Candidate;
use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder, Mnemonic, immediate};
use crate::executable::{Executable, SectionFlags};

/// Results from one pass over the executable's code sections.
///
/// See [`collect`] for how the scan fills these.
pub(super) struct ScanOutcome {
    /// Indirect jumps that may dispatch through a table, kept in
    /// section/offset scan order.
    pub(super) candidates: Vec<Candidate>,

    /// Addresses that might be code and might reveal a path around a check:
    ///
    /// * the entry point,
    /// * symbol addresses,
    /// * code section bases,
    /// * decoded branch/call targets.
    ///
    /// The table finder uses these to avoid trimming a table when a known path
    /// can skip its check.
    pub(super) entries: Vec<u64>,

    /// Starts of `cmp` instructions found by linear decoding:
    ///
    /// * they mark instruction starts,
    /// * they do not show that the code runs,
    /// * they do not rule out another reading of the bytes.
    pub(super) comparisons: Vec<u64>,
}

/// Finds table jumps and the targets of decoded branches and calls.
///
/// Adds relative branch/call targets as code entries.
///
/// # Arguments
///
/// * `executable` - the loaded guest executable.
/// * `decoder` - decodes the section bytes as we go.
/// * `buffer` - scratch space for the current instruction, reused per decode.
///
/// # Returns
///
/// The scan's [`ScanOutcome`].
///
/// # Remarks
///
/// Walks every code section one byte at a time; any `0xFF` byte might
/// be an indirect `jmp` through a table.
pub(super) fn collect(
    executable: &Executable,
    decoder: &Decoder,
    buffer: &mut DecodeBuffer,
) -> ScanOutcome {
    // Start with the entry point and symbols, then add results from each section.
    let mut candidates = Vec::new();
    let mut comparisons = Vec::new();
    let mut entries = vec![executable.entry_point.get()];
    entries.extend(executable.symbols.iter().map(|symbol| symbol.address.get()));

    for section in &executable.sections {
        // Only code sections can contain table jumps.
        if !section.flags.contains(SectionFlags::CODE) {
            continue;
        }

        // Get this section's bytes from the input file.
        let bytes = section.file_bytes(&executable.data);
        entries.push(section.base.get());

        // Check each byte so we can find jumps inside inline data too.
        let mut next_instruction = 0;
        for offset in 0..bytes.len() {
            // When the previous instruction ended here, decode the next one.
            if offset == next_instruction {
                // Decode one instruction.
                match buffer.decode_single_instruction(decoder, &bytes[offset..]) {
                    DecodeOutcome::Ok(insn) => {
                        if insn.mnemonic() == Mnemonic::CMP {
                            comparisons.push(section.base.get() + offset as u64);
                        }

                        // Remember where the next instruction starts.
                        next_instruction += insn.length();

                        // A relative first operand gives a branch or call target.

                        // Zydis marks the displacement as relative for these
                        // branch forms: jmp, call, jcc, loop, jecxz, and xbegin.
                        if let Some(operand) = immediate(&insn.operands()[0])
                            && operand.is_relative
                        {
                            let target = (section.base.get() + next_instruction as u64)
                                .wrapping_add(operand.value)
                                as u32;
                            entries.push(u64::from(target));
                        }
                    }
                    DecodeOutcome::Failed { .. } => next_instruction += 1,
                }
            }

            // An 0xFF byte might start a table jump.
            if bytes[offset] == 0xFF
                && let Some(candidate) = Candidate::at(
                    executable,
                    decoder,
                    buffer,
                    bytes,
                    offset,
                    section.base.get(),
                )
            {
                candidates.push(candidate);
            }
        }
    }

    comparisons.sort_unstable();
    comparisons.dedup();

    ScanOutcome {
        candidates,
        entries,
        comparisons,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::executable;
    use super::*;
    use rstest::rstest;

    // ── Core behaviour ───────────────────────────────────────────────────────

    #[rstest]
    #[case::before_dispatch(1, 0)]
    #[case::partial_dispatch(7, 0)]
    #[case::complete_dispatch(8, 1)]
    fn collect_should_bound_dispatch_reads_when_virtual_size_exceeds_storage(
        #[case] raw_size: u32,
        #[case] expected: usize,
    ) {
        // Arrange: the file keeps a dispatch past .text's stored bytes.
        //
        // The virtual tail past `raw_size` is zero-filled at runtime,
        // but the file still holds bytes there. Reads must stop at
        // `raw_size`, not at the virtual size.
        //
        // ```text
        // .text: (virtual size 8; storage ends at raw_size)
        //   0x11000: 90                    nop
        //   0x11001: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        // .rdata:
        //   0x11400: 00 00 00 00           dd 0 ; one mapped slot
        // ```
        //
        // - raw_size 1: storage ends before the jmp's 0xFF.
        // - raw_size 7: truncated jmp; must not decode.
        // - raw_size 8: whole jmp stored; one candidate.
        let text = [0x90, 0xff, 0x24, 0x85, 0, 0x14, 1, 0];
        let mut executable = executable(&text, &[0; 4]);
        executable.sections[0].raw_size = raw_size;
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // Act.
        let outcome = collect(&executable, &decoder, &mut buffer);

        // Assert.
        assert_eq!(outcome.candidates.len(), expected);
    }

    #[test]
    fn collect_should_find_dispatch_inside_another_instruction() {
        // Okay, this one needs some explanation. Here it is.
        //
        // It is generally NOT safe to assume that a function ONLY
        // contains code in a recompiler. This test exists to assert
        // that.
        //
        // It's technically possible to encounter something like this in code:
        // ```text
        // .text:
        //   0x11000: 74 04                 je 0x11006
        //   0x11002: 60 10 01 00           dd 0x11060 ; data inlined mid-code
        //   0x11006: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        // ```
        //
        // It's possible to have data in the middle of code/instructions,
        // like with `0x11002`.
        //
        // Problem is, a linear instruction decoder would not be able to
        // tell that this is data, and thus decode invalid instructions:
        // - `60` -> `pushad`
        // - `10 01` -> `adc [ecx], al`
        // - `00 FF` -> `add bh, bh`
        //
        // That FF right there is the first byte of the real instruction,
        // `jmp dword [eax*4 + 0x11400]`.
        //
        // Which is why it is so incredibly important to ensure we also
        // scan byte by byte to catch such demons.

        // Arrange:
        //
        // Two valid instruction encodings overlap:
        // - `jb`  at 0x11004: [0F 82 FF 24 85 00]
        // - `jmp` at 0x11006: [FF 24 85 00 14 01 00]
        //
        // The jump starts inside the branch's displacement.
        //
        // ```text
        // .text:
        //   0x11000: 90 90 90 90           nop padding
        //   0x11004: 0F 82 FF 24 85 00     jb rel32
        //   0x11006: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        // .rdata:
        //   0x11400: 00 00 00 00           dd 0 ; one mapped slot
        // ```
        //
        // Linear decoding skips to 0x1100A; byte-by-byte scanning must find
        // the jump at 0x11006.
        let mut text = [0x90; 0x0D];
        text[4..13].copy_from_slice(&[
            0x0F, 0x82, // 0x11004: jb rel32 (its immediate is the next four bytes)
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // 0x11006: jmp [eax*4 + 0x11400]
        ]);
        let executable = executable(&text, &[0; 4]);
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // Act.
        let outcome = collect(&executable, &decoder, &mut buffer);

        // Assert
        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(outcome.candidates[0].jump, 0x11006);
        assert_eq!(outcome.candidates[0].address, 0x11400);
    }

    #[rstest]
    #[case::unmapped_table(false)]
    #[case::data_section(true)]
    fn collect_should_reject_unmapped_or_non_code_dispatches(#[case] in_data: bool) {
        // Arrange: both instruction encodings are valid; only their mappings differ.
        //
        // ```text
        // unmapped_table:
        //   .text  0x11008          : FF 24 85 00 00 90 00  jmp dword [eax*4 + 0x900000]
        //   .rdata 0x11400          : 00 00 00 00           one slot; nothing mapped at 0x900000
        //
        // data_section:
        //   .text  0x11000..=0x1100F: 90 x16                only NOPs
        //   .rdata 0x11400          : 90                    nop
        //          0x11401          : FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        // ```
        //
        // Both dispatches have complete instruction bytes, so rejection tests
        // unmapped table storage or DATA classification.
        let mut text = [0x90; 0x10];
        let rdata = if in_data {
            vec![
                0x90, // nop: dispatch at .rdata offset 1
                0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
            ]
        } else {
            text[8..15].copy_from_slice(&[
                0xFF, 0x24, 0x85, 0x00, 0x00, 0x90, 0x00, // jmp [eax*4 + 0x900000]
            ]);
            vec![0; 4]
        };
        let executable = executable(&text, &rdata);
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // Act.
        let outcome = collect(&executable, &decoder, &mut buffer);

        // Assert:
        // Neither valid encoding becomes a candidate under these mappings.
        assert!(outcome.candidates.is_empty());
    }
}
