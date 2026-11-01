//! Switch jump-table definition.

use super::register::Register;
use serde::Deserialize;

/// A jump table (switch statement) in the game code.
///
/// A `[[switch]]` entry in the config:
///
/// ```toml
/// [[switch]]
/// register = "edx"
/// base = 0x1234
/// labels = [0x100, 0x200, 0x300]
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct SwitchTable {
    /// Register used in the `jmp` instruction.
    pub register: Register,

    /// Jump targets of the table, in table order.
    pub labels: Vec<u32>,

    /// Guest address of the `jmp` instruction.
    pub base: u64,
}

impl SwitchTable {
    /// Creates a switch table from the given fields.
    ///
    /// # Arguments
    ///
    /// - `register`: register used in the `jmp` instruction
    /// - `labels`: jump targets of the table, in table order
    /// - `base`: guest address of the `jmp` instruction the table
    ///   dispatches from (the map key)
    pub fn new(register: Register, labels: Vec<u32>, base: u64) -> Self {
        Self {
            register,
            labels,
            base,
        }
    }
}
