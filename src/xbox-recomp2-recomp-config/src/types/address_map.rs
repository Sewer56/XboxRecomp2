//! A map from guest addresses to values of any type `V`.
use super::FunctionAddress;
use ahash::HashMap; // go brrr

/// A map from guest addresses to values.
///
/// Usually used from configs, like this.
///
/// ```toml
/// [fixes]
/// functions = [
///   { address = 0x1000, size = 0x50 },
///   { address = 0x1000, size = 0x80 },
/// ]
/// ```
///
/// Where in the config, we use a `Vec<Function>` or simlar, and then insert
/// into a new [`AddressMap`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressMap<V> {
    entries: HashMap<FunctionAddress, V>,
}

impl<V> AddressMap<V> {
    /// Inserts `key` -> `value`, overwriting any existing entry for `key`.
    ///
    /// # Arguments
    ///
    /// - `key`: the guest address key
    /// - `value`: the value to store for `key`
    pub fn insert_or_overwrite(&mut self, key: FunctionAddress, value: V) {
        self.entries.insert(key, value);
    }

    /// Returns the value stored for `key`.
    ///
    /// # Arguments
    ///
    /// - `key`: the guest address to look up
    ///
    /// # Returns
    ///
    /// The value stored for `key`, or [`None`] when absent.
    pub fn get(&self, key: FunctionAddress) -> Option<&V> {
        self.entries.get(&key)
    }

    /// Entry count.
    ///
    /// # Returns
    ///
    /// The number of entries in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the map holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates over the map's entries.
    ///
    /// # Returns
    ///
    /// An iterator over `(address, value)` pairs in unspecified order.
    pub fn iter(&self) -> impl Iterator<Item = (FunctionAddress, &V)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }
}
