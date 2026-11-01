//! Finds switch-table targets, then drops those past the index check unless
//! code skips the check.
//!
//! See "Discover switch tables" in `doc/docs/function-discovery.md` for
//! examples.

use crate::decode::{DecodeBuffer, Decoder};
use crate::executable::{Executable, SwitchTable};
use bounds_check::BoundsCheck;
use scan::ScanOutcome;

mod bounds_check;
mod candidate;
mod scan;

/// Finds switch tables in scan order and keeps their targets in slot order.
///
/// See `doc/docs/function-discovery.md` for more details.
///
/// # Arguments
///
/// * `executable` - the loaded guest executable.
///
/// # Returns
///
/// Every table found.
///
/// # Remarks
///
/// See "Important Limitation" in `doc/docs/function-discovery.md`.
pub(super) fn discover_switch_tables(executable: &Executable) -> Vec<SwitchTable> {
    // Find possible table jumps, likely code entries, and comparison starts.
    let decoder = Decoder::new();
    let mut buffer = DecodeBuffer::new();
    let ScanOutcome {
        mut candidates, // indirect jumps that may use a table
        mut entries,    // addresses that may be code
        comparisons,    // comparison starts found by linear decoding
    } = scan::collect(executable, &decoder, &mut buffer);

    // Preparation: recognise possible `Direct Index Guard & Remap Array` etc.
    // boundaries (i.e. Step 2/3 in doc/docs/function-discovery.md)
    let mut starts = Vec::with_capacity(candidates.len());
    for candidate in &mut candidates {
        candidate.bounds =
            BoundsCheck::find(executable, &decoder, &mut buffer, candidate, &comparisons);

        starts.push(candidate.address);
    }

    starts.sort_unstable();
    starts.dedup();

    // Step 1 (doc/docs/function-discovery.md): collect as many switch cases as possible
    // using the fallback rules; before we trim.
    let mut tables: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            SwitchTable::new(
                candidate.register,
                candidate.labels(executable, &starts),
                candidate.jump,
            )
        })
        .collect();

    // Important Limitation (see doc/docs/function-discovery.md)
    // We now need to collect all destinations from branches.
    // When:
    //
    // ```asm
    //    other_code:
    //        mov ecx, 10
    //        jmp edx                ; edx points to `dispatch`
    //    checked_code:
    //        cmp ecx, 7
    //        ja  default_case
    //    dispatch:
    //        jmp [ecx*4 + targets]  ; other_code reads slot 10
    // ```
    //
    // So, here we collect the destinations from the initial scan of the switch
    // tables, just in case one switch table points at another.
    //
    // Notably, `entries` also contain locations like entry point, section
    // starts, etc. alongside the `jmp` destinations we want.
    //
    // But that's fine, our results are unaffected.
    entries.extend(
        tables
            .iter()
            .flat_map(|table| table.labels.iter().map(|&label| u64::from(label))),
    );
    entries.sort_unstable();
    entries.dedup();

    // Keep the fallback targets if a known entry lands inside the bounds check.
    for (candidate, table) in candidates.iter().zip(&mut tables) {
        if let Some(bounds) = candidate.bounds
            && bounds.can_trim(&entries)
        {
            table.labels.truncate(bounds.slots);
        }
        table.labels.shrink_to_fit();
    }

    tables
}

#[cfg(test)]
mod tests {
    //! Check switch scanning, target bounds, and guard bypasses with synthetic images.

    use super::*;
    use crate::config::Register;
    use crate::executable::{Section, SectionFlags};
    use rstest::rstest;

    // Shared data-only fixtures. Instruction bytes always stay in their tests.

    /// Maps .text at 0x11000 and .rdata at 0x11400, using each slice's length.
    ///
    /// - File offsets are 0x500 and 0x1400 respectively, deliberately
    ///   distinct from guest addresses.
    /// - Callers provide NOP padding where they need code addresses without
    ///   meaningful case bodies.
    pub(super) fn executable(text: &[u8], rdata: &[u8]) -> Executable {
        let mut data = vec![0; 0x1400 + rdata.len()];
        data[0x500..0x500 + text.len()].copy_from_slice(text);
        data[0x1400..].copy_from_slice(rdata);

        Executable {
            size: data.len() as u32,
            data,
            base: 0x10000u64.into(),
            entry_point: 0x11000u64.into(),
            address_limit: Some(0x8000_0000u64.into()),
            sections: vec![
                Section::new(
                    ".text",
                    0x11000u64,
                    text.len() as u32,
                    SectionFlags::CODE,
                    0x500,
                ),
                Section::new(
                    ".rdata",
                    0x11400u64,
                    rdata.len() as u32,
                    SectionFlags::DATA,
                    0x1400,
                ),
            ],
            ..Executable::default()
        }
    }

    // ── Discovery and mapped storage ─────────────────────────────────────────

    #[rstest]
    #[case::distant_code(SectionFlags::CODE, 4, &[0x11030, 0x20000, 0x11034])]
    #[case::non_code(SectionFlags::DATA, 4, &[0x11030])]
    #[case::zero_filled_code(SectionFlags::CODE, 0, &[0x11030])]
    fn discovery_should_collect_targets_until_first_non_code_section(
        #[case] flags: SectionFlags,
        #[case] raw_size: u32,
        #[case] expected_targets: &[u32],
    ) {
        // Arrange: (note: only the distant section's CODE/DATA flag changes)
        //
        // ```text
        // .text:
        //   0x11000..=0x11007: 90 x8                 nop padding
        //   0x11008          : FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //   0x1100F..=0x11037: 90 x41                nop padding; targets 0x11030, 0x11034 sit here
        // .rdata:
        //   0x11400          : 30 10 01 00           dd 0x11030
        //   0x11404          : 00 00 02 00           dd 0x20000
        //   0x11408          : 34 10 01 00           dd 0x11034
        // .far:
        //   0x20000..=0x20003: 00 x4                 declared section; no instructions supplied
        // ```
        //
        // - A distant code target is accepted without decoding its stored bytes.
        // - A data or zero-filled target stops collection and excludes later slots.
        let mut text = [0x90; 0x38];
        text[8..15].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
        ]);
        let mut executable = executable(&text, &table_bytes(&[0x11030, 0x20000, 0x11034]));
        executable.sections.push(Section {
            raw_size,
            ..Section::new(".far", 0x20000u64, 4, flags, 0)
        });

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].labels, expected_targets);
    }

    #[rstest]
    #[case::source_before_guard(8, 40)]
    #[case::source_after_guard(40, 8)]
    fn discovery_should_keep_all_destination_slots_when_source_is_trimmed(
        #[case] source: usize,
        #[case] guarded: usize,
    ) {
        // Arrange
        //
        // A's guard trims its table to one slot, dropping the pointer that
        // enters B past its guard. That pointer must still stop B's table
        // being trimmed from three slots to two.
        //
        // ```text
        // .text:
        //   0x11008: 83 F8 00              cmp eax, 0
        //   0x1100B: 77 30                 ja 0x1103D
        //   0x1100D: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]  ; A (-> B)
        //   0x11028: 83 F8 01              cmp eax, 1
        //   0x1102B: 77 30                 ja 0x1105D
        //   0x1102D: FF 24 85 08 14 01 00  jmp dword [eax*4 + 0x11408]  ; B
        //   NOPs elsewhere through 0x11073 cover the branch and table targets.
        // .rdata:
        //   0x11400: 70 10 01 00           dd 0x11070  ; A slot 0
        //   0x11404: 2D 10 01 00           dd 0x1102D  ; A slot 1 -> B dispatch
        //   0x11408: 60 10 01 00           dd 0x11060  ; B slot 0
        //   0x1140C: 64 10 01 00           dd 0x11064  ; B slot 1
        //   0x11410: 68 10 01 00           dd 0x11068  ; B slot 2
        // ```
        let mut text = [0x90; 0x74];
        text[source..source + 12].copy_from_slice(&[
            0x83, 0xF8, 0x00, // cmp eax, 0
            0x77, 0x30, // ja 0x11000 + source + 0x35
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
        ]);
        text[guarded..guarded + 12].copy_from_slice(&[
            0x83, 0xF8, 0x01, // cmp eax, 1
            0x77, 0x30, // ja 0x11000 + guarded + 0x35
            0xFF, 0x24, 0x85, 0x08, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11408]
        ]);
        let executable = executable(
            &text,
            &table_bytes(&[
                0x11070,
                0x11000 + guarded as u32 + 5,
                0x11060,
                0x11064,
                0x11068,
            ]),
        );

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        assert_eq!(tables.len(), 2);
        let source_table = tables
            .iter()
            .find(|table| table.base == 0x11000 + source as u64 + 5)
            .unwrap();
        let guarded_table = tables
            .iter()
            .find(|table| table.base == 0x11000 + guarded as u64 + 5)
            .unwrap();
        assert_eq!(source_table.labels, [0x11070]);
        assert_eq!(guarded_table.labels, [0x11060, 0x11064, 0x11068]);
    }

    // A byte map from one table must not limit another table.

    #[test]
    fn discovery_should_keep_all_slots_when_neighbour_remap_overlaps_table() {
        // Arrange
        // A's third pointer is also B's four-byte remap. Recognising B's remap
        // must not stop A's fallback collection before that third slot.
        //
        // ```text
        // .text:
        //   0x11008: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]  ; A
        //   0x11040: 83 F9 03              cmp ecx, 3
        //   0x11043: 77 60                 ja 0x110A5  ; default case
        //   0x11045: 0F B6 81 08 14 01 00  movzx eax, byte [ecx + 0x11408]
        //   0x1104C: FF 24 85 20 14 01 00  jmp dword [eax*4 + 0x11420]  ; B
        //   NOPs elsewhere through 0x110FF cover the branch and table targets.
        // .rdata:
        //   0x11400: E0 10 01 00           dd 0x110E0  ; A slot 0
        //   0x11404: E4 10 01 00           dd 0x110E4  ; A slot 1
        //   0x11408: E8 10 01 00           dd 0x110E8  ; A slot 2, also B's remap
        //   0x1140C..=0x1141F: 00 x20      zero padding; stops A's fallback
        //   0x11420..=0x117C3: .. x932     dd 0x110E0, 233 times  ; B slots 0..232
        // ```
        //
        // B maps inputs 0..=3 through bytes E8 10 01 00, giving slots 232, 16, 1, 0.
        // Its largest slot index is 232, so B needs 233 slots (max(remap) + 1).
        // Those same bytes still form A's valid third pointer, 0x110E8.
        let mut text = [0x90; 256];
        text[8..15].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // A: jmp [eax*4 + 0x11400]
        ]);
        text[64..83].copy_from_slice(&[
            0x83, 0xF9, 0x03, // 0x11040: cmp ecx, 3
            0x77, 0x60, // 0x11043: ja 0x110A5
            0x0F, 0xB6, 0x81, 0x08, 0x14, 0x01, 0x00, // movzx eax, byte [ecx + 0x11408]
            0xFF, 0x24, 0x85, 0x20, 0x14, 0x01, 0x00, // B: jmp [eax*4 + 0x11420]
        ]);
        let labels = [0x110E0, 0x110E4, 0x110E8];
        let mut rdata = table_bytes(&labels);
        rdata.resize(0x20, 0);
        rdata.extend_from_slice(&table_bytes(&[0x110E0; 233])); // B slots 0..232
        let executable = executable(&text, &rdata);

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: A keeps all three pointers; B keeps all 233 remapped slots.
        assert_eq!(tables[0].labels, labels);
        assert_eq!(tables[1].labels.len(), 233);
    }

    #[rstest]
    #[case::checked(false, 97)]
    #[case::bypassed(true, 98)]
    fn discovery_should_keep_pointer_shaped_remap_when_guard_is_bypassed(
        #[case] bypass: bool,
        #[case] expected_count: usize,
    ) {
        // Arrange
        // - Remap bytes map input values to table slots.
        // - Here, those bytes also form a valid code pointer.
        //
        // ```text
        // .text:
        //   0x11000..=0x11001: EB 13                    jmp short 0x11015  ; bypass guard and load
        //   0x11002..=0x11007: 90 x6                    nop padding
        //   0x11008          : 83 F8 03                 cmp eax, 3
        //   0x1100B          : 77 60                    ja 0x1106D
        //   0x1100D          : 0F B6 0C 05 84 15 01 00  movzx ecx, byte [eax + 0x11584]
        //   0x11015          : FF 24 8D 00 14 01 00     jmp dword [ecx*4 + 0x11400]
        //   NOPs elsewhere through 0x1106F cover the branch and table targets.
        // .rdata:
        //   0x11400..=0x11583: .. x388                  dd 0x11060, 97 times  ; slots 0..96
        //   0x11584          : 60 10 01 00              db 96, 16, 1, 0       ; input -> slot
        // ```
        //
        // - The largest remap index is 96, requiring 97 slots. All 97 exist.
        // - The same bytes, read as a little-endian pointer, give 0x11060.
        //   That is a valid code target, but these bytes are not a table entry.
        let mut text = [0x90; 0x70];
        if bypass {
            text[..2].copy_from_slice(&[
                0xEB, 0x13, // 0x11000: jmp short 0x11015, bypassing the guard
            ]);
        }
        text[8..28].copy_from_slice(&[
            0x83, 0xF8, 0x03, // 0x11008: cmp eax, 3
            0x77, 0x60, // 0x1100B: ja 0x1106D
            0x0F, 0xB6, 0x0C, 0x05, 0x84, 0x15, 0x01, 0x00, // movzx ecx, byte [eax+0x11584]
            0xFF, 0x24, 0x8D, 0x00, 0x14, 0x01, 0x00, // 0x11015: jmp [ecx*4 + 0x11400]
        ]);
        let labels = vec![0x11060; 97];
        let mut rdata = table_bytes(&labels);
        rdata.extend_from_slice(&[0x60, 0x10, 0x01, 0]); // remap, also dd 0x11060
        let executable = executable(&text, &rdata);

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: the checked case excludes slot 97; uncertainty retains it.
        assert_eq!(tables[0].labels, vec![0x11060; expected_count]);
    }

    // Bypass evidence must survive discovery and trimming.

    #[test]
    fn discovery_should_preserve_fallback_across_bypass_chain() {
        // This is that 'edge case' described in `doc/docs/function-discovery.md`
        //
        // We enter 0x11008 (A) with an index greater than 0.
        // But that index is out of bounds for B and C which do `cmp eax, 0`.
        //
        // However the tables of B and C have additional slots that can only
        // be reached when called by A.

        // Arrange
        //
        // ```text
        // .text:
        //   0x11008: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]  ; A  (-> B)
        //   0x11020: 83 F8 00              cmp eax, 0
        //   0x11023: 77 30                 ja 0x11055
        //   0x11025: FF 24 85 04 14 01 00  jmp dword [eax*4 + 0x11404]  ; B  (-> C)
        //   0x11040: 83 F8 00              cmp eax, 0
        //   0x11043: 77 30                 ja 0x11075
        //   0x11045: FF 24 85 10 14 01 00  jmp dword [eax*4 + 0x11410]  ; C
        //   NOPs elsewhere through 0x1108F cover the branch and table targets.
        // .rdata:
        //   0x11400: 25 10 01 00           dd 0x11025  ; A -> B dispatch
        //   0x11404: 80 10 01 00           dd 0x11080  ; B slot 0
        //   0x11408: 84 10 01 00           dd 0x11084  ; B slot 1
        //   0x1140C: 45 10 01 00           dd 0x11045  ; B slot 2 -> C dispatch
        //   0x11410: 88 10 01 00           dd 0x11088  ; C slot 0
        //   0x11414: 8C 10 01 00           dd 0x1108C  ; C slot 1
        // ```
        //
        // Both guards suggest one slot, but the bypasses make trimming unsafe.
        let mut text = [0x90; 0x90];
        text[8..15].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // A: jmp [eax*4 + 0x11400]
        ]);
        text[32..44].copy_from_slice(&[
            0x83, 0xF8, 0x00, // 0x11020: cmp eax, 0
            0x77, 0x30, // 0x11023: ja 0x11055
            0xFF, 0x24, 0x85, 0x04, 0x14, 0x01, 0x00, // B: jmp [eax*4 + 0x11404]
        ]);
        text[64..76].copy_from_slice(&[
            0x83, 0xF8, 0x00, // 0x11040: cmp eax, 0
            0x77, 0x30, // 0x11043: ja 0x11075
            0xFF, 0x24, 0x85, 0x10, 0x14, 0x01, 0x00, // C: jmp [eax*4 + 0x11410]
        ]);
        let executable = executable(
            &text,
            &table_bytes(&[0x11025, 0x11080, 0x11084, 0x11045, 0x11088, 0x1108C]),
        );

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        assert_eq!(tables.len(), 3);
        assert_eq!(tables[1].labels, [0x11080, 0x11084, 0x11045]); // not trimmed to 1
        assert_eq!(tables[2].labels, [0x11088, 0x1108C]); // not trimmed to 1
    }

    // Do not trim when a guard-looking byte sequence cannot be trusted.
    #[rstest]
    #[case::interior_comparison(&[
        0xBA, 0x83, 0xF8, 0x01, 0x77, // 0x11008: mov edx, 0x7701F883
        0x90, // 0x1100D: nop
    ])]
    #[case::padding_hides_conflict(&[
        0x83, 0xF8, 0x02, // 0x11008: cmp eax, 2
        0xBA, 0x00, 0x00, 0x90, 0x90, // 0x1100B: mov edx, 0x90900000
        0xBB, 0x83, 0xF8, 0x01, 0x90, // 0x11010: mov ebx, 0x9001F883
        0x77, 0x60, // 0x11015: ja 0x11077
    ])]
    #[case::remap_mutation(&[
        0x83, 0xF8, 0x01, // 0x11008: cmp eax, 1
        0x77, 0x60, // 0x1100B: ja 0x1106D
        0xC6, 0x05, 0x0C, 0x14, 0x01, 0x00, 0x02, // 0x1100D: mov byte [0x1140C], 2
        0x0F, 0xB6, 0x80, 0x0C, 0x14, 0x01, 0x00, // 0x11014: movzx eax, byte [eax + 0x1140C]
    ])]
    fn discovery_should_preserve_fallback_when_guard_is_unreliable(#[case] prefix: &[u8]) {
        // Arrange
        // Each case can reach slot 2, despite bytes that suggest a two-slot table.
        //
        // ```text
        // .text (interior_comparison):
        //   0x11008: BA 83 F8 01 77        mov edx, 0x7701F883
        //   0x1100D: 90                    nop
        //   0x1100E: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //
        //   Misleading decoding inside the mov:
        //   0x11009: 83 F8 01              cmp eax, 1
        //   0x1100C: 77 90                 ja 0x10F9E
        // ```
        //
        // - The actual code never checks `eax`
        // - With `eax = 2`, it uses slot 2.
        //
        // - Decoding inside the `mov` produces an apparent `cmp eax, 1` and `ja`.
        //   Those bytes belong to the constant and the following `nop`.
        // - The CPU does not run that check on this path. Keep slot 2.
        //
        // ```text
        // .text (padding_hides_conflict):
        //   0x11008: 83 F8 02              cmp eax, 2
        //   0x1100B: BA 00 00 90 90        mov edx, 0x90900000
        //   0x11010: BB 83 F8 01 90        mov ebx, 0x9001F883
        //   0x11015: 77 60                 ja 0x11077
        //   0x11017: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //
        //   Misleading decoding inside the second mov:
        //   0x11011: 83 F8 01              cmp eax, 1
        //   0x11014: 90                    nop
        // ```
        //
        // - The real check is `cmp eax, 2`.
        // - Both `mov` instructions preserve `eax` and the flags.
        // - With `eax = 2`, `ja` falls through to slot 2.
        // - Decoding inside the second `mov` gives `cmp eax, 1`. With the later
        //   `ja`, this looks like a bound allowing only slots 0 and 1.
        //
        // - The constants contain 0x90 bytes that look like `nop` padding.
        //   They must not stop the search before it finds the real comparison.
        //
        // - The apparent bounds disagree. Keep all three fallback slots.
        //
        // ```text
        // .text (remap_mutation):
        //   0x11008: 83 F8 01              cmp eax, 1
        //   0x1100B: 77 60                 ja 0x1106D
        //   0x1100D: C6 05 0C 14 01 00 02  mov byte [0x1140C], 2
        //   0x11014: 0F B6 80 0C 14 01 00  movzx eax, byte [eax + 0x1140C]
        //   0x1101B: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        // ```
        //
        // The remap at 0x1140C maps inputs to table slot numbers.
        //
        // - In the file, inputs 0 and 1 map to slots 0 and 1.
        // - At runtime, `eax = 0` passes the check. The store sets remap[0] to 2.
        //   Then `movzx` loads 2 into `eax`, so the table jump uses slot 2.
        // - The file's remap bytes do not match the runtime lookup.
        //   Keep slot 2 rather than trim based on those bytes.
        //
        // ```text
        // .rdata (all cases):
        //   0x11400: E0 10 01 00           dd 0x110E0  ; slot 0
        //   0x11404: E4 10 01 00           dd 0x110E4  ; slot 1
        //   0x11408: E8 10 01 00           dd 0x110E8  ; slot 2
        //   0x1140C: 00 01 00 00           db 0, 1, 0, 0  ; remap before execution
        // ```
        let mut text = [0x90; 256];
        text[8..8 + prefix.len()].copy_from_slice(prefix);
        let jump = 8 + prefix.len();
        text[jump..jump + 7].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
        ]);
        let labels = [0x110E0, 0x110E4, 0x110E8];
        let mut rdata = table_bytes(&labels);
        rdata.extend_from_slice(&[0, 1, 0, 0]);
        let executable = executable(&text, &rdata);

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: keep all three fallback slots, including the reachable slot 2.
        assert_eq!(tables[0].labels, labels);
    }

    #[rstest]
    // 0 is a non-code segment/target, so we stop.
    #[case::stop_at_non_code_target(&[0x11008, 0x11009, 0], &[0x11008, 0x11009])]
    // 0x9000_37E9 is outside of userspace, so we stop.
    #[case::invalid_first_target(&[0x9000_37E9], &[])]
    fn discovery_should_retain_mapped_table_and_valid_targets(
        #[case] slots: &[u32],
        #[case] expected_targets: &[u32],
    ) {
        // Arrange
        //
        // ```text
        // .text:
        //   0x11000: 90                    nop
        //   0x11001: FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //   0x11008: 90                    nop  ; slot-0 target
        //   0x11009: 90                    nop  ; slot-1 target
        // .rdata:
        //   0x11400: .. x4                 dd slots... (see cases)
        // ```
        let mut text = [0x90; 0x0A];
        text[1..8].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
        ]);
        let executable = executable(&text, &table_bytes(slots));

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        assert_eq!(
            tables,
            [SwitchTable::new(
                Register::Eax,
                expected_targets.to_vec(),
                0x11001
            )]
        );
    }

    #[rstest]
    #[case::unguarded(false)]
    #[case::guard_exceeds_neighbour(true)]
    fn discovery_should_stop_at_next_table_start(#[case] guarded: bool) {
        // Arrange: the first table's would-be slot 2 is another table's slot 0.
        //
        // ```text
        // .text:
        //   0x11000..=0x11007: 90 x8                 nop padding
        //   0x11008          : 83 F8 02              cmp eax, 2  ; guarded case only
        //   0x1100B          : 77 30                 ja 0x1103D  ; guarded case only
        //   0x1100D          : FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //   0x11028          : FF 24 85 08 14 01 00  jmp dword [eax*4 + 0x11408]
        //   0x1102F..=0x1106B: 90 x61                nop padding; targets 0x11060..0x11068 sit here
        // .rdata:
        //   0x11400          : 60 10 01 00           dd 0x11060  ; first table
        //   0x11404          : 64 10 01 00           dd 0x11064  ; first table
        //   0x11408          : 68 10 01 00           dd 0x11068  ; second table
        // ```
        //
        // - The unguarded case replaces the cmp/ja bytes with nops.
        let mut text = [0x90; 0x6C];
        if guarded {
            text[8..13].copy_from_slice(&[
                0x83, 0xF8, 0x02, // 0x11008: cmp eax, 2 (requests three slots)
                0x77, 0x30, // 0x1100B: ja 0x1103D
            ]);
        }
        text[13..20].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // 0x1100D: jmp [eax*4 + 0x11400]
        ]);
        text[40..47].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x08, 0x14, 0x01, 0x00, // 0x11028: jmp [eax*4 + 0x11408]
        ]);
        let executable = executable(&text, &table_bytes(&[0x11060, 0x11064, 0x11068]));

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: neither case consumes the neighbour's slot.
        assert_eq!(
            tables,
            [
                SwitchTable::new(Register::Eax, vec![0x11060, 0x11064], 0x1100D),
                SwitchTable::new(Register::Eax, vec![0x11068], 0x11028),
            ]
        );
    }

    #[rstest]
    #[case::virtual_end(4, 8)]
    #[case::stored_end(8, 4)]
    #[case::partial_stored_slot(8, 7)]
    fn discovery_should_stop_at_section_end_when_file_has_more_slots(
        #[case] virtual_size: u32,
        #[case] raw_size: u32,
    ) {
        // Arrange: two valid pointers, but only one inside .rdata.
        //
        // ```text
        // .text:
        //   0x11000..=0x11007: 90 x8                 nop padding
        //   0x11008          : FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //   0x1100F..=0x11037: 90 x41                nop padding; targets 0x11030, 0x11034 sit here
        // .rdata:
        //   0x11400          : 30 10 01 00           dd 0x11030
        // .null:
        //   0x11404          : 34 10 01 00           dd 0x11034, past .rdata's end
        // ```
        //
        // - .rdata is sized to one slot; we shrink it to 4 bytes.
        // - .null has another table, separate from .rdata.
        let mut text = [0x90; 0x38];
        text[8..15].copy_from_slice(&[
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // jmp [eax*4 + 0x11400]
        ]);
        let mut executable = executable(&text, &table_bytes(&[0x11030, 0x11034]));
        executable.sections[1].size = virtual_size;
        executable.sections[1].raw_size = raw_size;

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        // Only the first slot is fully stored and virtually mapped.
        assert_eq!(tables[0].labels, [0x11030]);
    }

    // ── Guard validation and neighbouring storage boundaries ─────────────────

    #[rstest]
    #[case::smaller_bound(1, 2)]
    #[case::equal_to_fallback(2, 3)]
    #[case::guard_exceeds_fallback(3, 3)]
    fn discovery_should_trim_only_valid_smaller_bounds(
        #[case] bound: u8,
        #[case] expected_count: usize,
    ) {
        // Arrange: without a guard, all three stored code pointers are collected.
        //
        // ```text
        // .text:
        //   0x11000..=0x11007: 90 x8                 nop padding
        //   0x11008          : 83 F8 ..              cmp eax, bound
        //   0x1100B          : 77 30                 ja 0x1103D  ; unsigned eax > bound skips table
        //   0x1100D          : FF 24 85 00 14 01 00  jmp dword [eax*4 + 0x11400]
        //   0x11014..=0x1106B: 90 x88                nop padding; targets 0x11060..0x11068 sit here
        // .rdata:
        //   0x11400          : 60 10 01 00           dd 0x11060
        //   0x11404          : 64 10 01 00           dd 0x11064
        //   0x11408          : 68 10 01 00           dd 0x11068
        // ```
        //
        // Tests:
        // - smaller_bound requests two slots (0, 1) : trim the third target.
        // - equal_to_fallback requests all 3        : nothing to trim.
        // - guard_exceeds_fallback requests 4       : but only 3 exist, keep all targets.
        let mut text = [0x90; 0x6C];
        text[8..20].copy_from_slice(&[
            0x83, 0xF8, bound, // 0x11008: cmp eax, bound
            0x77, 0x30, // 0x1100B: ja 0x1103D
            0xFF, 0x24, 0x85, 0x00, 0x14, 0x01, 0x00, // 0x1100D: jmp [eax*4 + 0x11400]
        ]);
        let labels = [0x11060, 0x11064, 0x11068];
        let executable = executable(&text, &table_bytes(&labels));

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].labels, labels[..expected_count]);
    }

    #[rstest]
    #[case::checked_path(false, 2)]
    #[case::bypassed_path(true, 3)]
    fn discovery_should_trim_remapped_slots_unless_bypassed(
        #[case] bypass: bool,
        #[case] expected_count: usize,
    ) {
        // Arrange
        // - Remap bytes map input values to table slots.
        // - Four inputs map to two slots; a third (unrelated) pointer follows.
        //
        // ```text
        // .text:
        //   0x11000..=0x11001: EB 13                    jmp short 0x11015  ; bypass path (optional)
        //   0x11002..=0x11007: 90 x6                    nop padding
        //   0x11008          : 83 F8 03                 cmp eax, 3
        //   0x1100B          : 77 60                    ja 0x1106D
        //   0x1100D          : 0F B6 0C 05 0C 14 01 00  movzx ecx, byte [eax + 0x1140C]
        //   0x11015          : FF 24 8D 00 14 01 00     jmp dword [ecx*4 + 0x11400]
        //   NOPs elsewhere through 0x1106F cover the branch and table targets.
        // .rdata:
        //   0x11400          : 60 10 01 00              dd 0x11060 ; slot 0
        //   0x11404          : 64 10 01 00              dd 0x11064 ; slot 1
        //   0x11408          : 68 10 01 00              dd 0x11068 ; unrelated pointer
        //   0x1140C          : 00 01 01 00              db 0, 1, 1, 0  ; input -> slot
        // ```
        //
        // - checked_path: No bypass that branches over `cmp eax, 3`
        //     - Trust the largest element in `input -> slot` as table cap.
        // - bypassed_path: We jump over `cmp eax, 3`, so can't trust it.
        //     - Keep all 3 elements from fallback approach.
        let mut text = [0x90; 0x70];
        if bypass {
            text[..2].copy_from_slice(&[
                0xEB, 0x13, // 0x11000: jmp short 0x11015 (dispatch)
            ]);
        }
        text[8..28].copy_from_slice(&[
            0x83, 0xF8, 0x03, // 0x11008: cmp eax, 3
            0x77, 0x60, // 0x1100B: ja 0x1106D
            0x0F, 0xB6, 0x0C, 0x05, 0x0C, 0x14, 0x01, 0x00, // movzx ecx, byte [eax+0x1140C]
            0xFF, 0x24, 0x8D, 0x00, 0x14, 0x01, 0x00, // 0x11015: jmp [ecx*4 + 0x11400]
        ]);
        let labels = [0x11060, 0x11064, 0x11068];
        let mut rdata = table_bytes(&labels);
        rdata.extend_from_slice(&[0, 1, 1, 0]); // remap: input -> slot
        let executable = executable(&text, &rdata);

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: trim the third pointer only when the remap cannot be bypassed.
        assert_eq!(tables[0].labels, labels[..expected_count]);
    }

    // ── Guard safety: trim only when the bytes can be trusted ────────────────

    #[test]
    fn discovery_should_trim_when_extra_instructions_preserve_guard() {
        // Arrange
        //
        // The fallback finds nine slots, but the guard allows only indices 0..=7.
        // Extra instructions between the `cmp`, `ja` and table jump keep that bound valid.
        //
        // ```text
        // .text:
        //   0x11008: 83 F8 07              cmp eax, 7
        //   0x1100B: 89 D3                 mov ebx, edx
        //   0x1100D: 77 60                 ja 0x1106F  ; default case
        //   0x1100F: 89 C1                 mov ecx, eax
        //   0x11011: 43                    inc ebx
        //   0x11012: FF 24 8D 00 14 01 00  jmp dword [ecx*4 + 0x11400]
        //   NOPs elsewhere through 0x110FF cover the branch and table targets.
        // .rdata:
        //   0x11400..=0x11423: .. x36      dd 0x110E0, 9 times  ; slots 0..8
        // ```
        //
        // - `mov ebx, edx` leaves eax and the comparison flags unchanged.
        // - `mov ecx, eax` copies the checked index into the table's index register.
        // - `inc ebx` changes flags only after `ja` has used them; ecx stays unchanged.
        let mut text = [0x90; 256];
        text[8..25].copy_from_slice(&[
            0x83, 0xF8, 0x07, // 0x11008: cmp eax, 7
            0x89, 0xD3, // 0x1100B: mov ebx, edx
            0x77, 0x60, // 0x1100D: ja 0x1106F
            0x89, 0xC1, // 0x1100F: mov ecx, eax
            0x43, // 0x11011: inc ebx
            0xFF, 0x24, 0x8D, 0x00, 0x14, 0x01, 0x00, // 0x11012: jmp [ecx*4 + 0x11400]
        ]);
        let labels = [0x110E0; 9];
        let executable = executable(&text, &table_bytes(&labels));

        // Act.
        let tables = discover_switch_tables(&executable);

        // Assert: keep slots 0..7 and trim the unreachable ninth slot (index 8).
        assert_eq!(tables[0].labels, labels[..8]);
    }

    fn table_bytes(labels: &[u32]) -> Vec<u8> {
        labels
            .iter()
            .flat_map(|label| label.to_le_bytes())
            .collect()
    }
}
