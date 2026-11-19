//! Address-sorted (low to high) table of guest [`Symbol`]s.
//!
//! # Remarks
//!
//! We fill the table during recompilation in 2 places:
//!
//! - import table (i.e. kernel thunks on Xbox, during XBE load)
//! - every discovered function during analysis
//!
//! Insertions are rare, so we append append and sort via
//! ([`SymbolTable::insert_batch`]); then do binary-search lookups.
//!
//! ## Note: Duplicates and zero-size symbols
//!
//! They can sometimes exist here.
//!
//! There's 3 places that 0-sized syms can come from:
//! - Failed Analysis
//! - Config entries with `size = 0`.
//! - Markers people may add in code down the road for arbitrary reasons.
//!
//! Just in case, I keep them here.

use super::address::VirtualAddress;
use super::symbol::Symbol;

/// A table of [`Symbol`]s, always sorted by address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolTable {
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    /// Creates an empty table.
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
        }
    }

    /// Inserts every symbol from `batch`.
    ///
    /// # Arguments
    ///
    /// * `batch` - symbols to append before re-sorting
    pub fn insert_batch(&mut self, batch: impl IntoIterator<Item = Symbol>) {
        self.symbols.extend(batch);
        self.symbols.sort_by_key(|symbol| symbol.address);
    }

    /// Finds the symbol exactly at `address`.
    /// The last with a non-zero size wins.
    ///
    /// # Arguments
    ///
    /// * `address` - the exact guest virtual address to look up
    ///
    /// # Returns
    ///
    /// The last symbol at `address` with a non-zero size, or
    /// [`None`] when there is no such symbol.
    pub fn find(&self, address: impl Into<VirtualAddress<u64>>) -> Option<&Symbol> {
        self.find_index(address.into())
            .map(|index| &self.symbols[index])
    }

    /// Gets table index of the at `address`.
    /// Last with a non-zero size wins.
    fn find_index(&self, address: VirtualAddress<u64>) -> Option<usize> {
        // Binary search first.
        let start = self
            .symbols
            .partition_point(|symbol| symbol.address < address);
        let mut match_index = None;

        // Now get symbol at the exact address, if any.
        for (offset, symbol) in self.symbols[start..].iter().enumerate() {
            if symbol.address != address {
                break;
            }

            // Last non-zero size wins.
            if symbol.size > 0 {
                match_index = Some(start + offset);
            }
        }

        match_index
    }

    /// Like [`SymbolTable::find`], just with mutable access.
    ///
    /// # Arguments
    ///
    /// * `address` - as for [`SymbolTable::find`]
    ///
    /// # Returns
    ///
    /// A mutable reference to the symbol [`SymbolTable::find`] would
    /// select, or [`None`].
    pub fn find_mut(&mut self, address: impl Into<VirtualAddress<u64>>) -> Option<&mut Symbol> {
        self.find_index(address.into())
            .map(|index| &mut self.symbols[index])
    }

    /// Iterates the table.
    ///
    /// # Returns
    ///
    /// An iterator over all symbols in address order.
    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }

    /// Iterates the table with mutable access.
    ///
    /// # Returns
    ///
    /// An iterator over all symbols in address order.
    ///
    /// # Remarks
    ///
    /// Don't change symbol addresses.
    /// Lookups rely on the table staying sorted by address.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Symbol> {
        self.symbols.iter_mut()
    }

    /// Symbol count.
    ///
    /// # Returns
    ///
    /// The number of symbols in the table.
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Whether this table is empty.
    ///
    /// # Returns
    ///
    /// `true` if the table holds no symbols, `false` otherwise.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn table_of(symbols: impl IntoIterator<Item = Symbol>) -> SymbolTable {
        let mut table = SymbolTable::new();
        table.insert_batch(symbols);
        table
    }

    fn names(table: &SymbolTable) -> Vec<&str> {
        table.iter().map(|symbol| symbol.name.as_ref()).collect()
    }

    #[rstest]
    #[case::inside(0x1004)]
    #[case::before(0x0FFF)]
    fn find_matches_start_address_only(#[case] address: u64) {
        // Arrange
        // `a` covers 0x1000..0x1100.
        let table = table_of([Symbol::function("a", 0x1000u64, 0x100)]);

        // Act
        let found = table.find(address);

        // Assert
        assert!(found.is_none());
    }

    #[rstest]
    #[case::zero_size(&[("a", 0)], None)]
    #[case::one(&[("a", 4)], Some("a"))]
    #[case::two(&[("a", 4), ("b", 4)], Some("b"))]
    #[case::zero_size_after(&[("a", 4), ("b", 0)], Some("a"))]
    #[case::zero_size_before(&[("a", 0), ("b", 4)], Some("b"))]
    fn find_picks_last_nonzero_size(
        #[case] symbols: &[(&str, u64)],
        #[case] expected: Option<&str>,
    ) {
        // Arrange
        // Every symbol sits at the same address.
        let table = table_of(
            symbols
                .iter()
                .map(|&(name, size)| Symbol::function(name, 0x1000u64, size)),
        );

        // Act
        let found = table.find(0x1000u64);

        // Assert
        assert_eq!(found.map(|symbol| symbol.name.as_ref()), expected);
    }

    #[test]
    fn insert_batch_sorts_by_address() {
        // Arrange
        // XBE load adds kernel imports first, at high addresses.
        let mut table = table_of([
            Symbol::function("kernel_a", 0x9000u64, 4),
            Symbol::function("kernel_b", 0x9100u64, 4),
        ]);

        // Act
        // Analysis then adds functions at lower addresses.
        table.insert_batch([
            Symbol::function("c", 0x2000u64, 4),
            Symbol::function("a", 0x1000u64, 4),
            Symbol::function("b", 0x1000u64, 4),
        ]);

        // Assert
        assert_eq!(names(&table), ["a", "b", "c", "kernel_a", "kernel_b"]);
    }

    #[test]
    fn insert_batch_keeps_existing_first() {
        // Arrange
        let mut table = table_of([Symbol::function("old", 0x1000u64, 4)]);

        // Act
        table.insert_batch([Symbol::function("new", 0x1000u64, 4)]);

        // Assert
        // So a later batch wins in `find`.
        assert_eq!(names(&table), ["old", "new"]);
    }
}
