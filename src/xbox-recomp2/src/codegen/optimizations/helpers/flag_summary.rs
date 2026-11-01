//! Stores what a function does with its caller's flags.

use crate::codegen::function::model::ALL_ARITHMETIC_FLAGS;
use crate::decode::CpuFlag;

/// What a function does with its caller's flags.
///
/// For example:
/// - `cmp eax, ebx; ret` reads no flags and keeps none.
/// - `mov eax, ebx; ret` keeps every flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) struct FlagSummary {
    /// Flags the function reads before writing them.
    ///
    /// ```text
    /// adc eax, 0   ; <- reads CF
    /// ret
    /// ```
    pub reads: CpuFlag,

    /// Flags that can reach a `ret` unchanged, plus [`Self::reads`].
    ///
    /// ```text
    /// inc ecx      ; <- doesn't write CF, so CF is kept
    /// ret
    /// ```
    pub keeps: CpuFlag,
}

impl FlagSummary {
    /// Assumes the function reads and keeps every flag.
    ///
    /// Use this for code we can't analyse.
    pub(in crate::codegen) const BARRIER: Self = Self {
        reads: ALL_ARITHMETIC_FLAGS,
        keeps: ALL_ARITHMETIC_FLAGS,
    };

    /// Assumes the function reads no flags and keeps none.
    pub(in crate::codegen) const CLOBBER: Self = Self {
        reads: CpuFlag::empty(),
        keeps: CpuFlag::empty(),
    };

    /// Flags needed before calling the function.
    ///
    /// # Arguments
    ///
    /// - `after`: flags needed after it returns.
    ///
    /// # Returns
    ///
    /// Flags it reads, plus flags in `after` that it keeps.
    pub(in crate::codegen) fn before(self, after: CpuFlag) -> CpuFlag {
        self.reads | (after & self.keeps)
    }
}
