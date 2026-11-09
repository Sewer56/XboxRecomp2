//! Switches for the optimization passes.

/// Turns optimization passes on or off.
///
/// # Usage
///
/// A pass only runs when [`enabled`] and its own switch are both `true`.
/// Use the `runs_*` methods rather than reading the fields.
///
/// [`enabled`]: Self::enabled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Optimizations {
    /// Master toggle.
    pub enabled: bool,

    /// Don't put the return address on the stack for function calls where possible.
    pub eliminate_return_slot: bool,

    /// Optimize away push+pop across function calls.
    pub eliminate_stack_args: bool,

    /// Trust `ret N` to put ESP back correctly in functions we can't fully follow.
    ///
    /// Used by passes that track the stack across calls.
    pub trust_callee_returns: bool,

    /// Translate x87 instructions to C, avoiding expensive FPU stack emulation.
    pub x87_lowering: bool,

    /// Only compute the eflags that later instructions actually read.
    pub eliminate_dead_flags: bool,

    /// Turn cmp + jump into a plain `if (a < b)`.
    pub fuse_compare_branch: bool,

    /// Find functions that use EBP as the frame pointer.
    pub find_ebp_frames: bool,

    /// Use a 32-bit divide for `idiv` after `cdq`.
    pub idiv32_after_cdq: bool,

    /// Work out which way string instructions like `rep movsd` run.
    pub find_string_direction: bool,
}

impl Optimizations {
    /// Optimizations with every single one disabled.
    /// Master switch on.
    ///
    /// # Returns
    ///
    /// Where [`enabled`] is `true` and each pass's own switch is `false`.
    ///
    /// [`enabled`]: Self::enabled
    pub const fn none() -> Self {
        Self {
            enabled: true,
            eliminate_return_slot: false,
            eliminate_stack_args: false,
            trust_callee_returns: false,
            x87_lowering: false,
            eliminate_dead_flags: false,
            fuse_compare_branch: false,
            find_ebp_frames: false,
            idiv32_after_cdq: false,
            find_string_direction: false,
        }
    }

    /// May we turn cmp + jump into a plain `if (a < b)`?
    pub fn runs_fuse_compare_branch(&self) -> bool {
        self.enabled && self.fuse_compare_branch
    }

    /// Are we allowed to leave the return address off the stack?
    pub fn runs_eliminate_return_slot(&self) -> bool {
        self.enabled && self.eliminate_return_slot
    }

    /// Are we allowed to eliminate stack argument writes and reads?
    pub fn runs_eliminate_stack_args(&self) -> bool {
        self.enabled && self.eliminate_stack_args
    }

    /// Do stack-tracking passes trust `ret N` in functions they can't fully follow?
    pub fn runs_trust_callee_returns(&self) -> bool {
        self.enabled && self.trust_callee_returns
    }

    /// Are we allowed to lower x87 code to C?
    pub fn runs_x87_lowering(&self) -> bool {
        self.enabled && self.x87_lowering
    }

    /// Are we allowed to compute only the flags later instructions read?
    pub fn runs_eliminate_dead_flags(&self) -> bool {
        self.enabled && self.eliminate_dead_flags
    }

    /// Do we look for functions that use EBP as the frame pointer?
    pub fn runs_find_ebp_frames(&self) -> bool {
        self.enabled && self.find_ebp_frames
    }

    /// May `idiv` after `cdq` use a 32-bit divide?
    pub fn runs_idiv32_after_cdq(&self) -> bool {
        self.enabled && self.idiv32_after_cdq
    }

    /// Do we work out which way string instructions run?
    pub fn runs_find_string_direction(&self) -> bool {
        self.enabled && self.find_string_direction
    }
}

impl Default for Optimizations {
    fn default() -> Self {
        Self {
            enabled: true,
            eliminate_return_slot: true,
            eliminate_stack_args: true,
            trust_callee_returns: false,
            x87_lowering: true,
            eliminate_dead_flags: true,
            fuse_compare_branch: true,
            find_ebp_frames: true,
            idiv32_after_cdq: true,
            find_string_direction: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Optimizations;
    use rstest::rstest;

    #[test]
    fn trust_callee_returns_should_not_need_stack_args() {
        // Arrange
        // Other passes that track the stack will use this switch too.
        let switches = Optimizations {
            eliminate_stack_args: false,
            trust_callee_returns: true,
            ..Optimizations::default()
        };

        // Act
        let actual = switches.runs_trust_callee_returns();

        // Assert
        assert!(actual);
    }

    #[rstest]
    #[case::all_on(true, true, true)]
    #[case::master_off(false, true, false)]
    #[case::own_switch_off(true, false, false)]
    fn optimization_passes_should_require_global_switch(
        #[values(
            Optimizations::runs_eliminate_return_slot,
            Optimizations::runs_eliminate_stack_args,
            Optimizations::runs_trust_callee_returns,
            Optimizations::runs_x87_lowering,
            Optimizations::runs_fuse_compare_branch,
            Optimizations::runs_eliminate_dead_flags,
            Optimizations::runs_find_ebp_frames,
            Optimizations::runs_idiv32_after_cdq,
            Optimizations::runs_find_string_direction
        )]
        runs: fn(&Optimizations) -> bool,
        #[case] enabled: bool,
        #[case] own_switch: bool,
        #[case] expected: bool,
    ) {
        // Arrange
        let switches = Optimizations {
            enabled,
            eliminate_return_slot: own_switch,
            eliminate_stack_args: own_switch,
            trust_callee_returns: own_switch,
            x87_lowering: own_switch,
            eliminate_dead_flags: own_switch,
            fuse_compare_branch: own_switch,
            find_ebp_frames: own_switch,
            idiv32_after_cdq: own_switch,
            find_string_direction: own_switch,
        };

        // Act
        let actual = runs(&switches);

        // Assert
        assert_eq!(actual, expected);
    }
}
