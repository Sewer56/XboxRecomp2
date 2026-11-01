//! Read possible function addresses from data tables.
//!
//! A table can have gaps.
//!
//! Here, both addresses (0x1080 and 0x1091) point to functions, so we find
//! both despite the zero and `0xFFFFFFFF` entries:
//!
//! ```text
//! 0x2000: dd 0x1080      ; first function
//! 0x2004: dd 0           ; skip (0 -> special case)
//! 0x2008: dd 0xFFFFFFFF  ; skip (u32::MAX -> special case)
//! 0x200C: dd 0x1091      ; second function
//! ```
//!
//! A bad address ends the scan instead.
//!
//! For example, if `1` is outside the image, we find
//! `0x1080` but never reach `0x1091`:
//!
//! ```text
//! 0x2000: dd 0x1080  ; first function
//! 0x2004: dd 1       ; stop here
//! 0x2008: dd 0x1091  ; not reached
//! ```

use super::{PointerScan, Resolution};
use crate::analysis::graph::{FunctionReference, ReferenceKind};
use crate::executable::SectionFlags;
use endian_writer::{EndianReader, LittleEndianReader};
use xbox_recomp2_executable::GuestAddress32;

impl PointerScan<'_> {
    /// Reads a table and adds references to possible functions.
    ///
    /// Each entry holds a four-byte, little-endian address. We check only the
    /// first instruction, so no 100% guarantee it's a real function start,
    /// only a guess mate.
    ///
    /// # Arguments
    ///
    /// - `site`: instruction that refers to this table.
    /// - `start`: four-byte-aligned address of the first item in the table.
    /// - `count`: how many items to read at most. [`None`] means no limit.
    /// - `result`: where to add the references.
    ///
    /// # Remarks
    ///
    /// We skip zero, [`u32::MAX`], and excluded or imported targets.
    /// Skipped entries still count towards the limit.
    ///
    /// We stop at an imported slot or another target that fails
    /// [`Self::is_callable`].
    pub(super) fn scan_table(
        &mut self,
        site: GuestAddress32,
        start: GuestAddress32,
        count: Option<u32>,
        result: &mut Resolution,
    ) {
        // Check address is 4 aligned.
        if !start.get().is_multiple_of(4) {
            return;
        }

        // Check cache
        let key = (start, count);
        if !self.tables.contains_key(&key) {
            // Get the table bytes.
            // Slice from first entry.
            let bytes = self
                .reader
                .window(start, SectionFlags::DATA, usize::MAX)
                .unwrap_or(&[]);

            // Read only complete four-byte entries, up to the requested count.
            let entry_count = (bytes.len() / 4).min(count.unwrap_or(u32::MAX) as usize);
            let mut reader = unsafe { LittleEndianReader::new(bytes.as_ptr()) };
            let mut targets = Vec::new();
            for index in 0..entry_count {
                let slot = GuestAddress32::from(start.get().wrapping_add((index as u32) * 4));

                // The import/thunk table itself is excluded here.
                if self.is_import(slot) {
                    break;
                }

                let target = unsafe { reader.read_u32() };
                let address = GuestAddress32::from(target);

                // Also skip excluded or imported function addresses
                // but keep reading later slots.
                if self.reader.is_excluded(address) || self.is_import(address) {
                    continue;
                }

                // Special case; don't end table on 0 or u32::MAX..
                // Because 0 can represent unimplemented virtual method.
                // u32::MAX can represent invalid/uninitialised entries.
                if target == 0 || target == u32::MAX {
                    continue;
                }

                // If instructions suggest it's likely not a function start
                // we can peace out too.
                if !self.is_callable(address) {
                    break;
                }

                targets.push(address);
            }

            targets.sort_unstable();
            targets.dedup();
            self.tables.insert(key, targets);
        }

        result
            .references
            .extend(self.tables[&key].iter().map(|&target| FunctionReference {
                site,
                target,
                kind: ReferenceKind::PossiblePointer,
            }));
    }
}

#[cfg(test)]
mod tests {
    //! Check where table reads stop and which entries they skip.

    use super::*;
    use crate::analysis::graph::test_helpers::base;
    use crate::analysis::pointer_tables::{
        test_helpers::resolution,
        test_helpers::{CALLBACK, TABLE, UNALIGNED, callback, executable, unaligned},
    };
    use crate::config::{FunctionAddress, InvalidInstructionSize, RecompilerConfig};
    use crate::executable::{ImportToken, ThunkAddress};
    use rstest::rstest;

    fn assert_targets(result: Resolution, expected: &[GuestAddress32]) {
        let references: Vec<_> = expected
            .iter()
            .map(|&target| FunctionReference {
                site: base(),
                target,
                kind: ReferenceKind::PossiblePointer,
            })
            .collect();
        assert_eq!(result.references, references);
    }

    // ── Slot reading ─────────────────────────────────────────────────────────

    #[rstest]
    #[case::section_end(4, 8, 0x108, vec![callback()])]
    #[case::partial_stored_slot(8, 6, 0x108, vec![callback()])]
    #[case::file_end(8, 8, 0x106, vec![callback()])]
    #[case::raw_start_missing(8, 8, 0x100, vec![])]
    fn reads_only_complete_slots(
        #[case] section_size: u32, // virtual mem size
        #[case] raw_size: u32,     // disk size
        #[case] raw_length: usize, // num bytes in file
        #[case] expected: Vec<GuestAddress32>,
    ) {
        // Section size, stored size, and file length each limit the scan.
        //
        // ```text
        //   0x1000: 68 00 20 00 00  push 0x2000
        //   0x1005: C3              ret
        //   0x2000: 80 10 00 00     dd 0x1080 ; file offsets 0x100..=0x103
        //   0x2004: 91 10 00 00     dd 0x1091 ; file offsets 0x104..=0x107
        // ```
        let code = [0x68, 0, 0x20, 0, 0, 0xc3];
        let mut executable = executable(&code, &[CALLBACK, UNALIGNED]);
        executable.sections[1].size = section_size;
        executable.sections[1].raw_size = raw_size;
        executable.data.truncate(raw_length);

        let result = resolution(&code, &executable, &RecompilerConfig::default());
        assert_targets(result, &expected);
    }

    #[rstest]
    #[case::mixed_slots(vec![0, CALLBACK, u32::MAX, 0, 0, UNALIGNED, 0])]
    #[case::long_gap([vec![CALLBACK], vec![0; 0x400], vec![UNALIGNED]].concat())]
    fn skips_empty_slots(#[case] entries: Vec<u32>) {
        // Zero and u32::MAX do not end the table, even across long gaps.
        //
        // ```text
        //   0x1000: 68 00 20 00 00  push 0x2000
        //   0x1005: C3              ret
        //
        // mixed_slots:
        //   0x2000: dd 0
        //   0x2004: dd 0x1080
        //   0x2008: dd 0xFFFFFFFF
        //   0x200C: dd 0
        //   0x2010: dd 0
        //   0x2014: dd 0x1091
        //   0x2018: dd 0
        //
        // long_gap:
        //   0x2000: dd 0x1080
        //   0x2004: dd 0 × 1024
        //   0x3004: dd 0x1091
        // ```
        let code = [0x68, 0, 0x20, 0, 0, 0xc3];
        let executable = executable(&code, &entries);

        let result = resolution(&code, &executable, &RecompilerConfig::default());
        assert_targets(result, &[callback(), unaligned()]);
    }

    #[test]
    fn continues_past_repeated_targets() {
        // A repeated address must not hide the later target.
        //
        // ```text
        //   0x1000: 68 00 20 00 00  push 0x2000
        //   0x1005: C3              ret
        //   0x2000: 80 10 00 00     dd 0x1080
        //   0x2004: 80 10 00 00     dd 0x1080 ; repeated target
        //   0x2008: 91 10 00 00     dd 0x1091
        // ```
        let code = [0x68, 0, 0x20, 0, 0, 0xc3];
        let executable = executable(&code, &[CALLBACK, CALLBACK, UNALIGNED]);

        let result = resolution(&code, &executable, &RecompilerConfig::default());
        assert_targets(result, &[callback(), unaligned()]);
    }

    #[rstest]
    #[case::before_section(TABLE - 4)]
    #[case::at_section_end(TABLE + 4)]
    #[case::unaligned_table(TABLE + 1)]
    fn rejects_invalid_table_starts(#[case] address: u32) {
        // Each address is outside the table or not four-byte aligned.
        //
        // ```text
        //   0x1000: 68 FC 1F 00 00  push 0x1FFC ; before_section
        //        or 68 04 20 00 00  push 0x2004 ; at_section_end
        //        or 68 01 20 00 00  push 0x2001 ; unaligned_table
        //   0x1005: C3              ret
        //   0x2000: 91 10 00 00     dd 0x1091
        // ```
        let mut code = vec![0x68];
        code.extend_from_slice(&address.to_le_bytes());
        code.push(0xc3);
        let executable = executable(&code, &[UNALIGNED]);

        let result = resolution(&code, &executable, &RecompilerConfig::default());
        assert_targets(result, &[]);
    }

    #[rstest]
    #[case::banned_target(CALLBACK, CALLBACK, vec![unaligned()])]
    #[case::banned_middle_of_prologue(CALLBACK + 2, CALLBACK + 2, vec![callback(), unaligned()])]
    #[case::banned_table(TABLE, TABLE + 3, vec![])]
    #[case::banned_second_slot(TABLE + 4, TABLE + 7, vec![callback()])]
    fn respects_exclusions(
        #[case] banned_start: u32,
        #[case] banned_end: u32,
        #[case] expected: Vec<GuestAddress32>,
    ) {
        let code = [0x68, 0, 0x20, 0, 0, 0xc3];
        let executable = executable(&code, &[CALLBACK, UNALIGNED]);
        let mut config = RecompilerConfig::default();
        config
            .banned_addresses
            .insert_or_overwrite(FunctionAddress(banned_start), FunctionAddress(banned_end));

        let result = resolution(&code, &executable, &config);
        assert_targets(result, &expected);
    }

    #[rstest]
    #[case::invalid_target(CALLBACK, false, vec![unaligned()])]
    #[case::import_target(CALLBACK, true, vec![unaligned()])]
    #[case::import_slot(TABLE, true, vec![])]
    fn skips_invalid_targets_and_stops_at_import_slots(
        #[case] address: u32,
        #[case] import: bool,
        #[case] expected: Vec<GuestAddress32>,
    ) {
        // Skip invalid or imported targets; stop at an imported slot.
        let code = [0x68, 0, 0x20, 0, 0, 0xc3];
        let mut executable = executable(&code, &[CALLBACK, UNALIGNED]);
        let mut config = RecompilerConfig::default();
        if import {
            executable
                .imports
                .add_thunk(ThunkAddress(address), ImportToken(1));
        } else {
            config
                .invalid_instructions
                .insert_or_overwrite(FunctionAddress(address), InvalidInstructionSize(1));
        }

        let result = resolution(&code, &executable, &config);
        assert_targets(result, &expected);
    }
}
