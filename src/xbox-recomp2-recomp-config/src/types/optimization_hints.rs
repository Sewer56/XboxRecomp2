//! Stuff the optimisation passes can't work out alone.

use super::AddressMap;

/// Stuff the optimisation passes can't work out alone.
///
/// # Remarks
///
/// Missing hints only cost speed.
/// Wrong hints break the game.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptimizationHints {
    /// What calling these functions does to the x87 stack.
    pub x87_stack_effects: AddressMap<X87StackEffect>,
}

/// What calling a function does to the x87 stack.
///
/// Example entry config:
///
/// ```toml
/// [optimization_hints]
/// x87_stack_effects = [
///   { address = 0x37DF64, stack_at_entry = 2, stack_at_exit = 1 }, # _CIpow
/// ]
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct X87StackEffect {
    /// x87 values on the stack at function entry. 8 max.
    pub stack_at_entry: u8,

    /// x87 values on the stack at function exit.
    /// - 1 if returns float in `st0`, else 0.
    pub stack_at_exit: u8,
}
