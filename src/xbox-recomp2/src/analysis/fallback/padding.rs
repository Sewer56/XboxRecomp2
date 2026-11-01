//! Find possible aligned function starts from padding.
//!
//! # Padding patterns
//!
//! We check for these patterns right before each 16-byte boundary.
//! If one matches, the boundary may be a function start.
//!
//! ```text
//! CC CC                 | int3; int3
//! C3 CC                 | ret; int3
//! C2 08 00 CC           | ret 8; int3
//! E9 xx xx xx xx CC     | jmp rel32; int3
//! FF E0 CC              | jmp eax; int3
//! ```
//!
//! The start must also look callable, see [`PointerScan::is_callable`].

use super::{code::file_backed_end, covered_starts::CoveredStarts};
use crate::analysis::graph::{BruteForceKind, DiscoveryGraph, FunctionDiscoverySource};
use crate::analysis::pointer_tables::PointerScan;
use crate::decode::{BranchType, DecodeBuffer, DecodeOutcome, Decoder, Mnemonic};
use crate::executable::{Executable, SectionFlags};
use xbox_recomp2_executable::GuestAddress32;

/// Compilers usually place function starts on 16-byte boundaries.
/// In order to optimise for CPU pipelining.
const FUNC_ALIGNMENT: u64 = 16;

/// Adds possible 16-byte-aligned function starts to `graph`.
///
/// # Arguments
///
/// - `executable`: the code to scan.
/// - `decoder`: checks indirect jumps.
/// - `scan`: reads code and checks if a start looks callable.
/// - `covered`: addresses we already know, so we skip them.
/// - `graph`: gets the starts we find.
pub(super) fn collect(
    executable: &Executable,
    decoder: &Decoder,
    scan: &mut PointerScan<'_>,
    covered: &CoveredStarts,
    graph: &mut DiscoveryGraph,
) {
    let mut buffer = DecodeBuffer::new();

    for section in &executable.sections {
        if !section.flags.contains(SectionFlags::CODE) {
            continue;
        }

        // Start at the first 16-byte boundary at or after the section base.
        let section_start = section.base.get();
        let section_end = file_backed_end(executable, section);
        let Some(mut address) = section_start
            .checked_add(FUNC_ALIGNMENT - 1)
            .map(|address| address & !(FUNC_ALIGNMENT - 1))
        else {
            continue;
        };

        while address < section_end {
            // Skip addresses already owned by known code or configured functions.
            if covered.contains(GuestAddress32::from(address as u32)) {
                address += FUNC_ALIGNMENT;
                continue;
            }

            // Look behind this boundary only after the first 16 bytes of the section.
            let has_padding = address >= section_start + FUNC_ALIGNMENT
                && match scan
                    .code_window(GuestAddress32::from((address - 2) as u32))
                    .get(..2)
                {
                    // Accept `ret; int3` or two `int3` bytes.
                    Some([0xc3 | 0xcc, 0xcc]) => true,
                    // Otherwise, look for `ret imm16` or a jump before the `int3`.
                    Some([_, 0xcc]) => {
                        matches!(
                            scan.code_window(GuestAddress32::from((address - 4) as u32))
                                .get(..4),
                            Some([0xc2, _, _, 0xcc])
                        ) || has_padded_jump(address as u32, decoder, &mut buffer, scan)
                    }
                    // A return or jump without a trailing `int3` is not enough.
                    _ => false,
                };

            // Keep the section base or a padded start if it looks callable.
            if (address == section_start || has_padding)
                && scan.is_callable(GuestAddress32::from(address as u32))
            {
                add_to_graph(graph, GuestAddress32::from(address as u32));
            }

            // Move to the next possible aligned start.
            address += FUNC_ALIGNMENT;
        }
    }
}

fn add_to_graph(graph: &mut DiscoveryGraph, entry: GuestAddress32) {
    graph.add_function(
        entry,
        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::Padding,
        },
        None,
    );
}

/// Checks for a `jmp` right before the `int3` at `boundary - 1`.
///
/// # Arguments
///
/// - `boundary`: candidate start, at least 16 bytes into the section.
///   The byte before it must be `CC`.
/// - `decoder`: for checking indirect jumps.
/// - `buffer`: scratch space for decoding.
/// - `scan`: reads code, stopping at exclusions.
///
/// # Returns
///
/// `true` for an `E9` jump or an unprefixed `FF /4` jump.
fn has_padded_jump(
    boundary: u32,
    decoder: &Decoder,
    buffer: &mut DecodeBuffer,
    scan: &PointerScan<'_>,
) -> bool {
    // A direct jump has four displacement bytes followed by the `int3`.
    // e.g. `E9 <imm32> CC`
    if matches!(
        scan.code_window(GuestAddress32::from(boundary - 6))
            .get(..6),
        Some([0xe9, _, _, _, _, 0xcc])
    ) {
        return true;
    }

    // Try each jump length so earlier exclusions cannot hide a shorter match.
    (2..=7).any(|jump_length| {
        // Read the jump and its `int3`. This fails if an exclusion is in the way.
        let Some(pattern_bytes) = scan
            .code_window(GuestAddress32::from(boundary - jump_length - 1))
            .get(..jump_length as usize + 1)
        else {
            return false;
        };

        // Bits 3..=5 in the ModR/M byte select FF /4, a near indirect jump.
        // Fuck, I forget the magic MODR/M encoding. Forgive me, for I hath
        // sinned by asking an LLM here.
        if pattern_bytes[0] != 0xff || pattern_bytes[1] & 0x38 != 0x20 {
            return false;
        }

        // Decode just the jump and require it to end before the padding.
        matches!(buffer.decode_single_instruction(decoder, &pattern_bytes[..jump_length as usize]),
            DecodeOutcome::Ok(insn)
                if insn.mnemonic() == Mnemonic::JMP
                    && insn.instruction().meta.branch_type == BranchType::NEAR
                    && insn.length() == jump_length as usize)
    })
}

#[cfg(test)]
mod tests {
    //! Check padding patterns and how exclusions affect possible starts.

    use super::super::{covered_starts, test_helpers::collect};
    use crate::analysis::graph::{DiscoveryGraph, test_helpers::base};
    use crate::analysis::pointer_tables::PointerScan;
    use crate::analysis::pointer_tables::test_helpers::{
        CALLBACK, callback, executable, unaligned,
    };
    use crate::config::{FunctionAddress, InvalidInstructionSize, RecompilerConfig};
    use crate::decode::Decoder;
    use rstest::rstest;

    // ── Padded jumps ─────────────────────────────────────────────────────────

    /// Valid near jumps ending in CC reveal 0x1080; other cases leave it out.
    #[rstest]
    #[case::direct(&[0xe9, 0, 0, 0, 0, 0xcc], true)] // jmp 0x107F; int3
    #[case::register(&[0xff, 0xe0, 0xcc], true)] // jmp eax; int3
    #[case::memory(&[0xff, 0x20, 0xcc], true)] // jmp [eax]; int3
    #[case::absolute(&[0xff, 0x25, 0, 0x20, 0, 0, 0xcc], true)] // jmp [0x2000]; int3
    #[case::disp8(&[0xff, 0x60, 4, 0xcc], true)] // jmp [eax+4]; int3
    #[case::sib_disp32(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], true)] // jmp [eax+ecx*4+0x2000]; int3
    #[case::bare_direct(&[0xe9, 0, 0, 0, 0], false)] // jmp 0x1080
    #[case::bare_indirect(&[0xff, 0xe0], false)] // jmp eax
    #[case::direct_nop(&[0xe9, 0, 0, 0, 0, 0x90], false)] // jmp 0x107F; nop
    #[case::indirect_nop(&[0xff, 0xe0, 0x90], false)] // jmp eax; nop
    #[case::far_indirect(&[0xff, 0x28, 0xcc], false)] // jmp far [eax]; int3
    #[case::indirect_call(&[0xff, 0xd0, 0xcc], false)] // call eax; int3
    #[case::invalid_register(&[0xff, 0xe8, 0xcc], false)] // invalid FF /5 register form
    #[case::truncated_displacement(&[0xff, 0xa0, 0, 0xcc], false)] // truncated jmp [eax+disp32]
    #[case::short_jump(&[0xeb, 0, 0xcc], false)] // jmp short 0x107F; int3
    fn collect_should_find_aligned_functions_when_preceded_by_padded_jumps(
        #[case] padding: &[u8],
        #[case] should_find_start: bool,
    ) {
        // Arrange.
        // Test the padding scan on its own, without references or tail jumps.
        //
        // ```text
        //   0x107D: FF E0  jmp eax
        //   0x107F: CC     int3     ; padding after the jump <- injected here
        //   0x1080: 55     push ebp ; aligned start: found
        // ```
        let mut executable = executable(&[0xc3], &[]);
        executable.data[0x70..0x80].fill(0x90);
        executable.data[0x80 - padding.len()..0x80].copy_from_slice(padding);

        let config = RecompilerConfig::default();
        let decoder = Decoder::new_compatibility_mode();
        let mut scan = PointerScan::new(&executable, &config, &decoder);
        let covered = covered_starts::CoveredStarts::new(&DiscoveryGraph::default(), &config);
        let mut graph = DiscoveryGraph::default();

        // Act.
        super::collect(&executable, &decoder, &mut scan, &covered, &mut graph);

        // Assert.
        // True means 0x1080 must be found; false means it must be absent.
        assert_eq!(graph.functions.contains_key(&callback()), should_find_start);
    }

    #[rstest]
    #[case::breakpoints([0xcc, 0xcc, 0xcc, 0xcc], true)]
    #[case::return_breakpoint([0xcc, 0xcc, 0xc3, 0xcc], true)]
    #[case::return_at_boundary([0xcc, 0xcc, 0xcc, 0xc3], false)]
    #[case::return_imm16([0xcc, 0xc2, 0x08, 0x00], false)]
    #[case::return_imm16_padding([0xc2, 0x08, 0x00, 0xcc], true)]
    #[case::return_imm16_large_padding([0xc2, 0x04, 0x01, 0xcc], true)]
    #[case::and_then_paddw([0x23, 0xc2, 0x0f, 0xfd], false)]
    #[case::and_then_movq([0x23, 0xc2, 0x0f, 0x6f], false)]
    #[case::return_nop([0xcc, 0xcc, 0xc3, 0x90], false)]
    #[case::return_imm16_nop([0xc2, 0x08, 0x00, 0x90], false)]
    #[case::return_imm16_across_boundary([0x90, 0x90, 0xc2, 0x08], false)]
    #[case::nop_padding([0x90, 0xc3, 0x90, 0xcc], false)]
    #[case::no_padding([0x90, 0x90, 0x90, 0x90], false)]
    fn collect_should_find_aligned_functions_when_preceded_by_padding(
        #[case] padding: [u8; 4],
        #[case] expected: bool,
    ) {
        // Arrange.
        // Change only the bytes at 0x107C..=0x107F in the shared layout.
        //
        // ```text
        //   0x1000: C3             ret
        //   0x107C: CC CC CC CC    int3 padding         ; breakpoints: accept 0x1080
        //        or CC CC C3 CC    ret; int3            ; return_breakpoint: accept
        //        or CC CC CC C3    ret, no int3         ; return_at_boundary: reject
        //        or CC C2 08 00    ret 8, no int3       ; return_imm16: reject
        //        or C2 08 00 CC    ret 8; int3          ; return_imm16_padding: accept
        //        or C2 04 01 CC    ret 260; int3        ; return_imm16_large_padding: accept
        //        or 23 C2 0F FD    and eax, edx; paddw  ; and_then_paddw: reject
        //        or 23 C2 0F 6F    and eax, edx; movq   ; and_then_movq: reject
        //        or CC CC C3 90    ret; nop             ; return_nop: reject
        //        or C2 08 00 90    ret 8; nop           ; return_imm16_nop: reject
        //        or 90 90 C2 08    ret 8 crosses 0x1080 ; return_imm16_across_boundary: reject
        //        or 90 C3 90 CC    nop; ret; nop; int3  ; nop_padding: reject
        //        or 90 90 90 90    nop padding          ; no_padding: reject
        // ```
        let mut executable = executable(&[0xc3], &[]);
        executable.data[0x7c..0x80].copy_from_slice(&padding);

        // Act.
        let result = collect(
            &executable,
            &RecompilerConfig::default(),
            &Decoder::new_compatibility_mode(),
        );

        // Assert.
        assert_eq!(result.contains(&callback()), expected);
        assert!(!result.contains(&unaligned()));
    }

    // ── Excluded padding bytes ───────────────────────────────────────────────

    #[rstest]
    #[case::direct(&[0xe9, 0, 0, 0, 0, 0xcc])]
    #[case::indirect(&[0xff, 0xe0, 0xcc])]
    fn collect_should_keep_padded_jumps_when_earlier_bytes_are_excluded(#[case] padding: &[u8]) {
        // Arrange.
        // Exclude the byte before the jump, leaving the actual pattern readable.
        //
        // ```text
        // Direct case:
        //   0x1079: CC              int3       ; excluded
        //   0x107A: E9 00 00 00 00  jmp 0x107F ; readable
        //   0x107F: CC              int3
        //   0x1080: 55              push ebp   ; keep this start
        //
        // Indirect case:
        //   0x107C: CC              int3       ; excluded
        //   0x107D: FF E0           jmp eax    ; readable
        //   0x107F: CC              int3
        //   0x1080: 55              push ebp   ; keep this start
        // ```
        let mut executable = executable(&[0xc3], &[]);
        executable.data[0x80 - padding.len()..0x80].copy_from_slice(padding);

        let mut config = RecompilerConfig::default();
        let excluded = FunctionAddress(CALLBACK - padding.len() as u32 - 1);
        config
            .banned_addresses
            .insert_or_overwrite(excluded, excluded);

        // Act.
        let result = collect(&executable, &config, &Decoder::new_compatibility_mode());

        // Assert.
        assert!(result.contains(&callback()));
    }

    #[rstest]
    #[case::return_opcode(&[0xc2, 8, 0, 0xcc], 4)]
    #[case::return_low(&[0xc2, 8, 0, 0xcc], 3)]
    #[case::return_high(&[0xc2, 8, 0, 0xcc], 2)]
    #[case::return_pad(&[0xc2, 8, 0, 0xcc], 1)]
    #[case::direct_opcode(&[0xe9, 0, 0, 0, 0, 0xcc], 6)]
    #[case::direct_disp0(&[0xe9, 0, 0, 0, 0, 0xcc], 5)]
    #[case::direct_disp1(&[0xe9, 0, 0, 0, 0, 0xcc], 4)]
    #[case::direct_disp2(&[0xe9, 0, 0, 0, 0, 0xcc], 3)]
    #[case::direct_disp3(&[0xe9, 0, 0, 0, 0, 0xcc], 2)]
    #[case::direct_pad(&[0xe9, 0, 0, 0, 0, 0xcc], 1)]
    #[case::indirect_opcode(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 8)]
    #[case::indirect_modrm(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 7)]
    #[case::indirect_sib(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 6)]
    #[case::indirect_disp0(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 5)]
    #[case::indirect_disp1(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 4)]
    #[case::indirect_disp2(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 3)]
    #[case::indirect_disp3(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 2)]
    #[case::indirect_pad(&[0xff, 0xa4, 0x88, 0, 0x20, 0, 0, 0xcc], 1)]
    fn collect_should_skip_starts_when_padding_bytes_are_excluded(
        #[case] padding: &[u8],
        #[case] distance: u32,
        #[values(false, true)] invalid: bool,
    ) {
        // Arrange.
        // Exclude one byte of an otherwise valid return or jump with padding.
        //
        // ```text
        //   0x107C: C2 08 00              ret 8
        //       or
        //   0x107A: E9 00 00 00 00        jmp 0x107F
        //       or
        //   0x1078: FF A4 88 00 20 00 00  jmp [eax + ecx*4 + 0x2000]
        //   0x107F: CC                    int3
        //   0x1080: 55                    push ebp ; skip this start
        // ```
        // Each case excludes the byte at 0x1080 - distance; 0x1000 stays eligible.
        let mut executable = executable(&[0xc3], &[]);
        executable.data[0x80 - padding.len()..0x80].copy_from_slice(padding);

        let address = CALLBACK - distance;
        let mut config = RecompilerConfig::default();
        if invalid {
            config
                .invalid_instructions
                .insert_or_overwrite(FunctionAddress(address), InvalidInstructionSize(1));
        } else {
            config
                .banned_addresses
                .insert_or_overwrite(FunctionAddress(address), FunctionAddress(address));
        }

        // Act.
        let result = collect(&executable, &config, &Decoder::new_compatibility_mode());

        // Assert.
        assert_eq!(result, [base()]);
    }
}
