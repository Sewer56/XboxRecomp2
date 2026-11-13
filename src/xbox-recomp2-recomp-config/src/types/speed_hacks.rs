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

    /// Treat `ffree` to work as a `pop` when possible.
    ///
    /// Benefits:
    /// - Speed!
    ///
    /// Side effects:
    /// - `top` can differ from real hw (x87 is a circular buffer).
    pub fast_x87_ffree: bool,

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

    /// Let the compiler merge, reorder or drop guest loads and stores.
    /// Enable `nonvolatile_stack` to also allow this for stack.
    pub nonvolatile_memory: bool,

    /// CMOVcc reads a memory source only when it moves.
    pub lazy_cmov_load: bool,

    /// Use native word size for guest addresses.
    /// Usually safe to use, just beware this will break 64-bit wraps.
    pub native_sized_addresses: bool,

    /// Run REP MOVS and REP STOS as `memmove` and `memset`.
    pub rep_memmove_memset: bool,

    /// Assume code we can't see never passes eflags across a call or return.
    pub clobber_eflags_for_unknown_code: bool,

    /// Keep the stack slots of leaf functions in C++ locals.
    /// Stack memory below ESP then differs from real hw after a return.
    pub stack_slots_in_locals: bool,

    /// Release spin locks with a regular store instead of a locked `xchg`.
    pub plain_lock_release: bool,

    /// `xchg` on memory at or above this guest address uses regular store.
    /// Use only if one thread takes locks there.
    pub single_thread_locks_from: u32,

    /// Debug check for `single_thread_locks_from`.
    /// Prints on error.
    pub check_single_thread_locks: bool,
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

    /// Do we treat `ffree` as a `pop` when possible?
    pub fn runs_fast_x87_ffree(&self) -> bool {
        self.enabled && self.fast_x87_ffree
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

    /// Do safe guest loads and stores skip `volatile`?
    pub fn runs_nonvolatile_memory(&self) -> bool {
        self.enabled && self.nonvolatile_memory
    }

    /// Does CMOVcc skip its memory read when it doesn't move?
    pub fn runs_lazy_cmov_load(&self) -> bool {
        self.enabled && self.lazy_cmov_load
    }

    /// Do guest addresses skip the 4 GB wrap?
    pub fn runs_native_sized_addresses(&self) -> bool {
        self.enabled && self.native_sized_addresses
    }

    /// Do REP MOVS and REP STOS run as `memmove` and `memset`?
    pub fn runs_rep_memmove_memset(&self) -> bool {
        self.enabled && self.rep_memmove_memset
    }

    /// Do calls and returns we can't follow clobber eflags?
    pub fn runs_clobber_eflags_for_unknown_code(&self) -> bool {
        self.enabled && self.clobber_eflags_for_unknown_code
    }

    /// Do leaf functions keep their stack slots in C++ locals?
    pub fn runs_stack_slots_in_locals(&self) -> bool {
        self.enabled && self.stack_slots_in_locals
    }

    /// The guest address at or above which `xchg` skips the locked instruction, if any.
    pub fn single_thread_locks_from(&self) -> Option<u32> {
        (self.enabled && self.single_thread_locks_from != 0)
            .then_some(self.single_thread_locks_from)
    }

    /// Do we report `single_thread_locks_from` locks that two threads use?
    pub fn runs_check_single_thread_locks(&self) -> bool {
        self.enabled && self.check_single_thread_locks
    }

    /// Do `xchg`s that store zero run as a plain store?
    pub fn runs_plain_lock_release(&self) -> bool {
        self.enabled && self.plain_lock_release
    }
}

impl Default for SpeedHacks {
    fn default() -> Self {
        Self {
            enabled: true,
            fast_trig: true,
            fast_x87_status: true,
            fast_x87_stack: true,
            fast_x87_ffree: true,
            fast_fscale: true,
            fast_sse_status: true,
            nonvolatile_stack: true,
            plain_sse_memory: true,
            nonvolatile_memory: true,
            lazy_cmov_load: true,
            native_sized_addresses: true,
            rep_memmove_memset: true,
            clobber_eflags_for_unknown_code: true,
            stack_slots_in_locals: true,
            plain_lock_release: true,
            single_thread_locks_from: 0,
            check_single_thread_locks: false,
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
            SpeedHacks::runs_fast_x87_ffree,
            SpeedHacks::runs_fast_fscale,
            SpeedHacks::runs_fast_sse_status,
            SpeedHacks::runs_nonvolatile_stack,
            SpeedHacks::runs_plain_sse_memory,
            SpeedHacks::runs_nonvolatile_memory,
            SpeedHacks::runs_lazy_cmov_load,
            SpeedHacks::runs_native_sized_addresses,
            SpeedHacks::runs_rep_memmove_memset,
            SpeedHacks::runs_clobber_eflags_for_unknown_code,
            SpeedHacks::runs_stack_slots_in_locals,
            SpeedHacks::runs_plain_lock_release,
            SpeedHacks::runs_check_single_thread_locks
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
            fast_x87_ffree: own_switch,
            fast_fscale: own_switch,
            fast_sse_status: own_switch,
            nonvolatile_stack: own_switch,
            plain_sse_memory: own_switch,
            nonvolatile_memory: own_switch,
            lazy_cmov_load: own_switch,
            native_sized_addresses: own_switch,
            rep_memmove_memset: own_switch,
            clobber_eflags_for_unknown_code: own_switch,
            stack_slots_in_locals: own_switch,
            plain_lock_release: own_switch,
            single_thread_locks_from: 0,
            check_single_thread_locks: own_switch,
        };

        // Act
        let actual = runs(&switches);

        // Assert
        assert_eq!(actual, expected);
    }
}
