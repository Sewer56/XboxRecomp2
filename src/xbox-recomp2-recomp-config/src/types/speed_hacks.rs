//! Switches for the speed hacks.

/// Turns speed hacks on or off.
///
/// A hack runs only when [`enabled`] and its own switch are both `true`.
/// Check with the `runs_*` methods.
///
/// [`enabled`]: Self::enabled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedHacks {
    /// Master toggle.
    pub enabled: bool,

    /// Use 64-bit instead of 80-bit precision for trig and 2^x.
    pub fast_trig: bool,

    /// Don't track the x87 overflow, divide-by-zero and precision-loss flags.
    pub fast_x87_status: bool,

    /// Don't check for x87 stack overflow and underflow.
    pub fast_x87_stack: bool,

    /// FSCALE uses a 64-bit path instead of 80-bit.
    /// Scales beyond 2^±1022 give wrong results.
    pub fast_fscale: bool,

    /// Don't track SSE exception flags in MXCSR.
    pub fast_sse_status: bool,

    /// Let the compiler keep guest stack values in registers.
    /// - `[esp+N]` by default.
    /// - `[ebp+N]` where possible if `find_ebp_frames` optimisation is enabled.
    pub nonvolatile_stack: bool,

    /// Let the compiler merge or drop packed SSE loads and stores.
    pub plain_sse_memory: bool,

    /// CMOVcc reads a memory source only when it moves.
    pub lazy_cmov_load: bool,

    /// Don't wrap STOS addresses around at 4 GB.
    pub unwrapped_string_addresses: bool,

    /// Assume code we can't see never passes eflags across a call or return.
    pub clobber_eflags_for_unknown_code: bool,
}

impl SpeedHacks {
    /// Do we use 64-bit instead of 80-bit precision for trig and 2^x?
    pub fn runs_fast_trig(&self) -> bool {
        self.enabled && self.fast_trig
    }

    /// Do we skip the x87 overflow, divide-by-zero and precision-loss flags?
    pub fn runs_fast_x87_status(&self) -> bool {
        self.enabled && self.fast_x87_status
    }

    /// Do we skip the x87 stack overflow and underflow checks?
    pub fn runs_fast_x87_stack(&self) -> bool {
        self.enabled && self.fast_x87_stack
    }

    /// Does FSCALE use the 64-bit path instead of 80-bit?
    pub fn runs_fast_fscale(&self) -> bool {
        self.enabled && self.fast_fscale
    }

    /// Do we skip the SSE exception flags?
    pub fn runs_fast_sse_status(&self) -> bool {
        self.enabled && self.fast_sse_status
    }

    /// Do guest stack accesses skip `volatile`?
    pub fn runs_nonvolatile_stack(&self) -> bool {
        self.enabled && self.nonvolatile_stack
    }

    /// Do packed SSE loads and stores skip `volatile`?
    pub fn runs_plain_sse_memory(&self) -> bool {
        self.enabled && self.plain_sse_memory
    }

    /// Does CMOVcc skip its memory read when it doesn't move?
    pub fn runs_lazy_cmov_load(&self) -> bool {
        self.enabled && self.lazy_cmov_load
    }

    /// Do REP STOS addresses skip the 4 GB wrap?
    pub fn runs_unwrapped_string_addresses(&self) -> bool {
        self.enabled && self.unwrapped_string_addresses
    }

    /// Do calls and returns we can't follow clobber eflags?
    pub fn runs_clobber_eflags_for_unknown_code(&self) -> bool {
        self.enabled && self.clobber_eflags_for_unknown_code
    }
}

impl Default for SpeedHacks {
    fn default() -> Self {
        Self {
            enabled: true,
            fast_trig: true,
            fast_x87_status: true,
            fast_x87_stack: true,
            fast_fscale: true,
            fast_sse_status: true,
            nonvolatile_stack: true,
            plain_sse_memory: true,
            lazy_cmov_load: true,
            unwrapped_string_addresses: true,
            clobber_eflags_for_unknown_code: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpeedHacks;
    use rstest::rstest;

    #[rstest]
    #[case::all_on(true, true, true)]
    #[case::master_off(false, true, false)]
    #[case::own_switch_off(true, false, false)]
    fn speed_hacks_should_require_global_switch(
        #[values(
            SpeedHacks::runs_fast_trig,
            SpeedHacks::runs_fast_x87_status,
            SpeedHacks::runs_fast_x87_stack,
            SpeedHacks::runs_fast_fscale,
            SpeedHacks::runs_fast_sse_status,
            SpeedHacks::runs_nonvolatile_stack,
            SpeedHacks::runs_plain_sse_memory,
            SpeedHacks::runs_lazy_cmov_load,
            SpeedHacks::runs_unwrapped_string_addresses,
            SpeedHacks::runs_clobber_eflags_for_unknown_code
        )]
        runs: fn(&SpeedHacks) -> bool,
        #[case] enabled: bool,
        #[case] own_switch: bool,
        #[case] expected: bool,
    ) {
        // Arrange
        let switches = SpeedHacks {
            enabled,
            fast_trig: own_switch,
            fast_x87_status: own_switch,
            fast_x87_stack: own_switch,
            fast_fscale: own_switch,
            fast_sse_status: own_switch,
            nonvolatile_stack: own_switch,
            plain_sse_memory: own_switch,
            lazy_cmov_load: own_switch,
            unwrapped_string_addresses: own_switch,
            clobber_eflags_for_unknown_code: own_switch,
        };

        // Act
        let actual = runs(&switches);

        // Assert
        assert_eq!(actual, expected);
    }
}
