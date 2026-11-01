//! Find function starts from tail-call jumps.

use super::code;
use crate::analysis::graph::{
    BruteForceKind, DiscoveryGraph, Flow, FunctionDiscoverySource, Instruction,
};
use crate::analysis::pointer_tables::PointerScan;
use crate::decode::{Decoder, Mnemonic};
use crate::executable::Executable;
use std::collections::BTreeSet;
use xbox_recomp2_executable::GuestAddress32;

struct Jump {
    site: GuestAddress32,
    target: GuestAddress32,
    section_start: GuestAddress32,
    section_end: u64,
}

/// Adds likely tail-call targets to `graph`.
///
/// We decode each code section once, from start to end.
/// If decoding fails, we skip one byte and retry.
///
/// We add a jump's target when:
///
/// - The jump is a direct, near `jmp`.
/// - The target is code, not an import.
/// - The target's first instruction decodes, is supported, and is not `int3`.
/// - No `C3` or `C2 xx 00` (return) bytes start in the 4 bytes after the jump.
/// - A known start sits at or before the jump, in the same section.
/// - The target is before that start, or at/after the next one.
///
/// See "Tail-jump scan" in `doc/docs/function-discovery.md` for background.
pub(super) fn collect(
    executable: &Executable,
    decoder: &Decoder,
    scan: &mut PointerScan<'_>,
    graph: &mut DiscoveryGraph,
) {
    // Decode the jumps once.
    // And on later pass, change the candidate ranges.
    let mut jumps = Vec::new();
    code::for_each_decoded_instruction(
        executable,
        decoder,
        scan,
        |address, insn, section, scan| {
            // No jump? Skip!
            if insn.mnemonic() != Mnemonic::JMP {
                return;
            }
            let instruction = Instruction::from_decoded(address, insn);
            let Some(target) = instruction.direct_target() else {
                return;
            };

            // Trim to file end
            let end = code::file_backed_end(executable, section);

            // A callable target is a useful clue...
            // unless nearby return bytes rule it out.
            if instruction.flow != Flow::Jump
                || !scan.is_callable(target)
                || followed_by_return(scan, instruction.end(), end)
            {
                return;
            }
            jumps.push(Jump {
                site: address,
                target,
                section_start: GuestAddress32::from(section.base.get() as u32),
                section_end: end,
            });
        },
    );

    // Okay. Now we got the jumps.
    // We're going to look through all the jumps and check following:
    // - Between section start and the next known start or section end
    let mut known: BTreeSet<_> = graph.functions.keys().copied().collect();

    loop {
        let before = known.len();
        jumps.retain(|jump| {
            let mut start = None;
            let mut end = jump.section_end;

            // A jump can enter another function or stay inside the same one.
            //
            // We don't want to mistake a loop or cleanup block for a function start.
            // We use the starts we already know to guess which function contains
            // the jump. A target outside its guessed boundaries may be another function.
            //
            // For example, we know starts at 0x1020 and 0x1040.
            //
            // If a jump at 0x1028 targets 0x1050. We assume the jump belongs
            // to the function starting at 0x1020, with 0x1040 as its boundary.
            //
            // ```text
            // 0x1020       0x1028       0x1040       0x1050
            // known start  jump site    next start   target
            // |------------------------)
            //      assumed function range
            // ```
            //
            // Here, 0x1050 is outside those boundaries, so we consider it a function start.
            // This is a clue, as we don't know where the function really ends.
            // If the section's readable end comes before the next start, use that end.
            for &known_start in known.range(jump.section_start..) {
                if known_start > jump.site {
                    // This is the first start after the jump.
                    end = u64::from(known_start.get()).min(jump.section_end);
                    break;
                }

                start = Some(known_start);
            }

            let Some(start) = start else {
                return true; // Keep this jump for another pass.
            };

            if jump.target < start || u64::from(jump.target.get()) >= end {
                add_tail_call(graph, jump);
                known.insert(jump.target);
                return false;
            }

            true
        });

        if known.len() == before {
            break;
        }
    }
}

fn add_tail_call(graph: &mut DiscoveryGraph, jump: &Jump) {
    graph.add_function(
        jump.target,
        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::TailCall { site: jump.site },
        },
        None,
    );
}

/// Checks whether return-like bytes follow a jump.
///
/// We look for `C3` or `C2 xx 00` starting in the first 4 bytes after the jump:
///
/// ```text
/// C3        ret       -> ignore the jump
/// C2 04 00  ret 4     -> ignore the jump
/// C2 04 01  ret 0x104 -> does not match
/// ```
fn followed_by_return(scan: &PointerScan<'_>, address: u64, section_end: u64) -> bool {
    if address >= section_end {
        return false;
    }

    let bytes = scan.code_window(GuestAddress32::from(address as u32));
    let bytes = &bytes[..bytes.len().min((section_end - address) as usize)];

    bytes
        .iter()
        .take(4)
        .enumerate()
        .any(|(index, &byte)| byte == 0xc3 || (byte == 0xc2 && bytes.get(index + 2) == Some(&0)))
}

#[cfg(test)]
mod tests {
    //! Check which direct jumps can reveal possible function starts.
    use super::super::test_helpers::collect as collect_fallback;
    use super::*;
    use crate::analysis::graph::test_helpers::{BASE, at, executable};
    use crate::config::{FunctionAddress, RecompilerConfig};
    use crate::executable::{ImportToken, Section, SectionFlags, ThunkAddress};
    use rstest::rstest;

    #[rstest]
    #[case::truncated_jump(0x7c, 0x11)]
    #[case::truncated_target(0x28, 0x7f)]
    fn skips_truncated_instructions(#[case] source: u32, #[case] target: u32) {
        // The file ends at 0x1080, in the middle of the jump or target instruction.
        //
        // ```text
        // truncated_jump:
        //   0x107C: E9 90 FF FF FF  jmp 0x1011 ; last byte is missing
        // truncated_target:
        //   0x1028: E9 52 00 00 00  jmp 0x107F
        //   0x107F: 6A FF           push -1    ; operand is missing
        // ```
        let mut executable = image(&[(source, target)]);
        executable.data.truncate(0x80);
        let result = scan(&executable, &RecompilerConfig::default(), &[0x20, 0x40]);
        assert_eq!(result, [0x20, 0x40]);
    }

    #[rstest]
    #[case::plain_return(&[0xc3], false)]
    #[case::stack_return(&[0xc2, 4, 0], false)]
    #[case::large_stack_return(&[0xc2, 4, 1], true)]
    fn checks_return_bytes_after_the_jump(#[case] after_jump: &[u8], #[case] expected: bool) {
        // We check these bytes even though execution jumps past them.
        //
        // ```text
        // 0x1028: E9 E4 FF FF FF  jmp 0x1011
        // 0x102D: C3              ret       ; reject the jump
        //      or C2 04 00        ret 4     ; reject the jump
        //      or C2 04 01        ret 0x104 ; does not match the byte pattern
        // ```
        let mut executable = image(&[(0x28, 0x11)]);
        executable.data[0x2d..0x2d + after_jump.len()].copy_from_slice(after_jump);
        let result = scan(&executable, &RecompilerConfig::default(), &[0x20, 0x40]);
        assert_eq!(result.contains(&0x11), expected);
    }

    #[test]
    fn skips_import_targets() {
        // 0x1028: E9 E4 FF FF FF  jmp 0x1011 ; known import thunk
        let mut executable = image(&[(0x28, 0x11)]);
        executable
            .imports
            .add_thunk(ThunkAddress(BASE + 0x11), ImportToken(1));
        let result = scan(&executable, &RecompilerConfig::default(), &[0x20, 0x40]);
        assert_eq!(result, [0x20, 0x40]);
    }

    #[test]
    fn skips_banned_targets() {
        // 0x1028: E9 E4 FF FF FF  jmp 0x1011 ; config bans this target
        let executable = image(&[(0x28, 0x11)]);
        let mut config = RecompilerConfig::default();
        let target = FunctionAddress(BASE + 0x11);
        config.banned_addresses.insert_or_overwrite(target, target);
        let result = scan(&executable, &config, &[0x20, 0x40]);
        assert_eq!(result, [0x20, 0x40]);
    }

    #[test]
    fn checks_again_after_adding_a_start() {
        // Adding 0x1030 splits the first range. The jump at 0x1010 then leaves it.
        //
        // ```text
        //   0x1010: E9 2B 00 00 00  jmp 0x1040 ; initially inside [0x1000, 0x1060)
        //   0x1030: 6A FF           push -1
        //   0x1032: C3              ret
        //   0x1040: 6A FF           push -1
        //   0x1042: C3              ret
        //   0x1068: E9 C3 FF FF FF  jmp 0x1030 ; outside [0x1060, section end)
        // ```
        let executable = image(&[(0x10, 0x40), (0x68, 0x30)]);
        // Use unsorted, repeated starts to check that the result is sorted and unique.
        let result = scan(&executable, &RecompilerConfig::default(), &[0x60, 0, 0x60]);
        assert_eq!(result, [0, 0x30, 0x40, 0x60]);
    }

    #[rstest]
    #[case::before_start(0x11, &[0x20, 0x40], &[0x11, 0x20, 0x40])]
    #[case::after_next_start(0x51, &[0x20, 0x40], &[0x20, 0x40, 0x51])]
    #[case::local_jump(0x21, &[0x20, 0x40], &[0x20, 0x40])]
    #[case::known_target(0x40, &[0x20, 0x40], &[0x20, 0x40])]
    #[case::no_known_start(0x11, &[], &[])]
    fn compares_targets_with_the_candidate_range(
        #[case] target: u32,
        #[case] starts: &[u32],
        #[case] expected: &[u32],
    ) {
        // With starts at 0x1020 and 0x1040, only targets outside that range are new.
        //
        // ```text
        // 0x1028: E9 E4 FF FF FF  jmp 0x1011 ; before the start: add it
        //      or E9 24 00 00 00  jmp 0x1051 ; past the next start: add it
        //      or E9 F4 FF FF FF  jmp 0x1021 ; inside the range: leave it out
        //      or E9 13 00 00 00  jmp 0x1040 ; already known: do not add it twice
        // ```
        // With no known start, we cannot assign the jump to a range.
        let executable = image(&[(0x28, target)]);
        let result = scan(&executable, &RecompilerConfig::default(), starts);
        assert_eq!(result, expected);
    }

    #[test]
    fn ends_the_range_at_the_source_section() {
        // The target is before the next start, but outside the jump's section.
        //
        // ```text
        // .text (file offset 0, ends at 0x1090):
        //   0x1028: E9 D4 00 00 00  jmp 0x1101
        // .other (file offset 0x90, code section):
        //   0x1100: 90              nop
        //   0x1101: 6A FF           push -1
        //   0x1103: C3              ret
        //   0x1104: 90              nop ; next known start
        // ```
        let mut executable = image(&[(0x28, 0x101)]);
        executable
            .data
            .extend_from_slice(&[0x90, 0x6a, 0xff, 0xc3, 0x90]);
        executable.sections.push(Section::new(
            ".other",
            u64::from(BASE + 0x100),
            5,
            SectionFlags::CODE,
            0x90,
        ));
        let result = scan(&executable, &RecompilerConfig::default(), &[0x20, 0x104]);
        assert_eq!(result, [0x20, 0x101, 0x104]);
    }

    #[test]
    fn finds_a_tail_target_without_padding() {
        // The padding scan finds 0x1060. The tail scan finds its unpadded target.
        //
        // ```text
        //   0x1000: CC              int3
        //   0x1011: 6A FF           push -1
        //   0x1013: C3              ret
        //   0x105E: CC CC           int3; int3
        //   0x1060: E9 AC FF FF FF  jmp 0x1011
        // ```
        let mut executable = image(&[(0x60, 0x11)]);
        executable.data[0] = 0xcc;
        executable.data[0x5e..0x60].fill(0xcc);
        let result = collect_fallback(
            &executable,
            &RecompilerConfig::default(),
            &Decoder::new_compatibility_mode(),
        );

        assert_eq!(result, [at(0x11), at(0x60)]);
    }

    fn image(jumps: &[(u32, u32)]) -> Executable {
        // Test addresses are offsets from BASE (0x1000) and also file offsets.
        // Put push -1; ret at each target and fill the rest with NOPs.
        let mut code = vec![0x90; 0x90];
        for &(_, target) in jumps {
            if let Some(bytes) = code.get_mut(target as usize..target as usize + 3) {
                bytes.copy_from_slice(&[0x6a, 0xff, 0xc3]);
            }
        }
        for &(site, target) in jumps {
            // E9's displacement is relative to the end of the five-byte jump.
            let site = site as usize;
            code[site] = 0xe9;
            code[site + 1..site + 5]
                .copy_from_slice(&(target as i32 - site as i32 - 5).to_le_bytes());
        }

        executable(&code)
    }

    fn scan(executable: &Executable, config: &RecompilerConfig, starts: &[u32]) -> Vec<u32> {
        let decoder = Decoder::new_compatibility_mode();
        let mut scan = PointerScan::new(executable, config, &decoder);
        let mut graph = DiscoveryGraph::default();
        for &start in starts {
            graph.add_function(
                at(start),
                FunctionDiscoverySource::BruteForce {
                    kind: BruteForceKind::Padding,
                },
                None,
            );
        }
        collect(executable, &decoder, &mut scan, &mut graph);

        graph
            .functions
            .into_keys()
            .map(|address| address.get() - BASE)
            .collect()
    }
}
