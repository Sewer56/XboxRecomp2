//! Guest symbols.

use super::address::VirtualAddress;

/// One guest symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Symbol name.
    pub name: Box<str>,

    /// Guest address of the symbol.
    pub address: VirtualAddress<u64>,

    /// Size of the symbol in bytes.
    pub size: u64,

    /// Symbol kind.
    pub kind: SymbolType,
}

/// Symbol kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    /// No kind assigned.
    None,

    /// A Section.
    Section,

    /// A Function.
    Function,

    /// A Comment.
    Comment,
}

impl Symbol {
    /// Creates a function symbol with the given name, address and size.
    ///
    /// # Arguments
    ///
    /// * `name` - symbol name
    /// * `address` - guest address of the symbol
    /// * `size` - size of the symbol in bytes
    pub fn function(
        name: impl Into<Box<str>>,
        address: impl Into<VirtualAddress<u64>>,
        size: u64,
    ) -> Self {
        Self {
            name: name.into(),
            address: address.into(),
            size,
            kind: SymbolType::Function,
        }
    }
}
