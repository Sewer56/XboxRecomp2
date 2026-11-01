//! Works out which arithmetic flags cross calls and returns, for every function.
//!
//! See `flag-boundaries.md` for more info.

use super::flag_summary::FlagSummary;
use super::model_index::ModelIndex;
use crate::analysis::FunctionGraph;
use crate::codegen::function::model::{ALL_ARITHMETIC_FLAGS, FunctionModel};
use crate::decode::CpuFlag;
use std::collections::BTreeMap;
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

mod builder;

/// What each function's callers and callees do with arithmetic flags.
pub(in crate::codegen::optimizations) struct FlagBoundaries {
    /// Map of function address to array index.
    address_to_index: BTreeMap<GuestAddress32, ModelIndex>,

    /// What each function does with its caller's flags.
    summaries: Vec<FlagSummary>,

    /// Flags each function's callers can read after it returns.
    at_return: Vec<CpuFlag>,

    /// What code we can't see does with flags, such as indirect calls.
    unknown: FlagSummary,
}

impl FlagBoundaries {
    /// Works out which flags cross each call and return in `models`.
    ///
    /// # Arguments
    ///
    /// - `models`: every function model.
    /// - `graph`: the discovered call graph.
    /// - `clobber_eflags_for_unknown_code`: whether the speed hack is on.
    /// - `diag`: where to write the summary line.
    pub(in crate::codegen::optimizations) fn build(
        models: &[FunctionModel],
        graph: &FunctionGraph,
        clobber_eflags_for_unknown_code: bool,
        diag: &mut dyn Write,
    ) -> Self {
        let flags = builder::build(models, graph, clobber_eflags_for_unknown_code);

        // Log how many functions return fewer flags.
        let narrowed = flags
            .at_return
            .iter()
            .filter(|&&needed| needed != ALL_ARITHMETIC_FLAGS)
            .count();

        let _ = writeln!(
            diag,
            "flag-boundaries: {narrowed} of {} functions return fewer flags",
            models.len()
        );
        flags
    }

    /// Flags callers can read after `entry` returns.
    ///
    /// # Returns
    ///
    /// Every flag if `entry` is unknown.
    pub(super) fn at_return(&self, entry: GuestAddress32) -> CpuFlag {
        self.address_to_index
            .get(&entry)
            .map_or(ALL_ARITHMETIC_FLAGS, |&function| self.at_return[function])
    }

    /// What calling or jumping to `target` does with flags.
    ///
    /// # Arguments
    ///
    /// - `target`: the address called or jumped to, or [`None`] if indirect.
    ///
    /// # Returns
    ///
    /// - The function's summary.
    /// - [`FlagSummary::BARRIER`] if `target` isn't the start of a function.
    /// - [`FlagSummary::BARRIER`] for [`None`], or [`FlagSummary::CLOBBER`] with
    ///   `clobber_eflags_for_unknown_code`.
    pub(super) fn flag_summary(&self, target: Option<GuestAddress32>) -> FlagSummary {
        match target {
            Some(target) => self
                .address_to_index
                .get(&target)
                .map_or(FlagSummary::BARRIER, |&callee| self.summaries[callee]),
            None => self.unknown,
        }
    }
}

impl Default for FlagBoundaries {
    /// Creates boundaries with no functions.
    fn default() -> Self {
        Self {
            address_to_index: BTreeMap::new(),
            summaries: Vec::new(),
            at_return: Vec::new(),
            unknown: FlagSummary::BARRIER,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::{executable, layout, program};
    use crate::config::RecompilerConfig;
    use rstest::rstest;

    const ENTRY: &[u8] = &[
        0xE8, 0x3B, 0, 0, 0, // call 0x1040
        0x39, 0xD1, //          cmp  ecx, edx
        0xC3, //                ret
    ];

    /// Builds [`ENTRY`], `caller` at 0x1020 and `called` at 0x1040.
    fn build_with_callee(
        caller: &[u8],
        called: &[u8],
        config: &RecompilerConfig,
        clobber_eflags_for_unknown_code: bool,
    ) -> FlagBoundaries {
        let code = layout(&[(0x1000, ENTRY), (0x1020, caller), (0x1040, called)]);
        build_from(&code, config, clobber_eflags_for_unknown_code)
    }

    /// Builds the boundaries for every function discovered in `code`.
    fn build_from(
        code: &[u8],
        config: &RecompilerConfig,
        clobber_eflags_for_unknown_code: bool,
    ) -> FlagBoundaries {
        let (graph, models) = program(&mut executable(code), config);
        FlagBoundaries::build(
            &models,
            &graph,
            clobber_eflags_for_unknown_code,
            &mut Vec::new(),
        )
    }

    /// Base common case.
    #[rstest]
    #[case::caller_overwrites(
        &[
            0xE8, 0x1B, 0, 0, 0,    // call 0x1040
            0x39, 0xD1,             // cmp  ecx, edx        ; <- replaces the callee's flags
            0xC3,                   // ret
        ],
        CpuFlag::empty(),
    )]
    #[case::caller_reads_zero(
        &[
            0xE8, 0x1B, 0, 0, 0,    // call 0x1040
            0x0F, 0x94, 0xC0,       // setz al              ; <- reads the callee's ZF
            0x39, 0xD1,             // cmp  ecx, edx
            0xC3,                   // ret
        ],
        CpuFlag::ZF,
    )]
    fn build_should_return_only_flags_known_callers_read(
        #[case] caller: &[u8],
        #[case] expected: CpuFlag,
    ) {
        // Arrange
        let called = [
            0x39, 0xD8, //               cmp  eax, ebx
            0xC3, //                     ret
        ];

        // Act
        let flags = build_with_callee(caller, &called, &RecompilerConfig::default(), false);

        // Assert
        assert_eq!(flags.at_return(0x1040.into()), expected);
    }

    /// A tail jump's target returns to our caller.
    #[test]
    fn build_should_pass_return_flags_through_tail_jumps() {
        // Arrange
        // 0x1060 is a function of its own because the entry also calls it.
        let entry = [
            0xE8, 0x3B, 0, 0, 0, //      call 0x1040
            0x0F, 0x94, 0xC0, //         setz al              ; <- reads ZF after the call
            0xE8, 0x53, 0, 0, 0, //      call 0x1060
            0x39, 0xD1, //               cmp  ecx, edx
            0xC3, //                     ret
        ];
        let tail = [
            0xE9, 0x1B, 0, 0, 0, //      jmp  0x1060          ; <- tail jump
        ];
        let target = [
            0x39, 0xD8, //               cmp  eax, ebx
            0xC3, //                     ret
        ];
        let code = layout(&[(0x1000, &entry), (0x1040, &tail), (0x1060, &target)]);

        // Act
        let flags = build_from(&code, &RecompilerConfig::default(), false);

        // Assert
        let at_return = flags.at_return(0x1060.into());
        assert!(at_return.contains(CpuFlag::ZF), "{at_return:?}");
    }

    // If a function calls itself; we treat that as all flags.
    // since we can't resolve.
    #[test]
    fn build_should_summarise_recursive_callee_as_clobber() {
        // Arrange
        let called = [
            0x85, 0xC0, //                   test eax, eax
            0x74, 0x06, //                   jz   done
            0x48, //                         dec  eax
            0xE8, 0xF6, 0xFF, 0xFF, 0xFF, // call 0x1040          ; <- calls itself
            0x39, 0xD8, //                   done: cmp eax, ebx
            0xC3, //                         ret
        ];

        // Act
        let flags = build_with_callee(&[], &called, &RecompilerConfig::default(), false);

        // Assert
        assert_eq!(
            flags.flag_summary(Some(0x1040.into())),
            FlagSummary::CLOBBER
        );
    }

    #[rstest]
    #[case::reads_carry(
        &[
            0x83, 0xD0, 0,          // adc  eax, 0
            0xC3,                   // ret
        ],
        CpuFlag::CF,
        CpuFlag::CF,
    )]
    #[case::keeps_all(
        &[
            0x89, 0xD8,             // mov  eax, ebx
            0xC3,                   // ret
        ],
        CpuFlag::empty(),
        ALL_ARITHMETIC_FLAGS,
    )]
    #[case::keeps_carry(
        &[
            0x40,                   // inc  eax             ; <- doesn't write CF
            0xC3,                   // ret
        ],
        CpuFlag::empty(),
        CpuFlag::CF,
    )]
    fn build_should_report_callee_reads_and_keeps(
        #[case] called: &[u8],
        #[case] reads: CpuFlag,
        #[case] keeps: CpuFlag,
    ) {
        // Act
        let flags = build_with_callee(&[], called, &RecompilerConfig::default(), false);

        // Assert
        assert_eq!(
            flags.flag_summary(Some(0x1040.into())),
            FlagSummary { reads, keeps }
        );
    }
}
