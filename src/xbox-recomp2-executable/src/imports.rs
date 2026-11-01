//! The import table! OS functions and functions from external DLLs!
//!
//! In the case of the Xbox; the kernel isn't statically linked.
//! (That was quite a surprise to me!)
//!
//! Games call it through a table of "thunks" in `.rdata`.
//! So a game does a call like this (Shadow the Hedgehog - NTSC-U):
//!
//! ```text
//! 0x002AC43E:  FF 15 E4 9A 42 00   ; call dword ptr [0x00429AE4]
//! ```
//!
//! The slot at `0x00429AE4` holds an index (a.k.a. 'ordinal'):
//!
//! ```text
//! 0x00429AE4:  0x80000115          ; flag bits + ordinal 0x115
//! ```
//!
//! And this is patched during game load time to point to correct
//! address of kernel function.
//!
//! This map is how the recompiler rewrites such calls.
//! We store the slot's address in here as a [`ThunkAddress`],
//! and the slot's value in there as an [`ImportToken`].
//!
//! This design should (hopefully), support Xbox binaries, Windows binaries,
//! and Linux ones. Not sure about others yet.

use ahash::{HashMap, HashSet};
use derive_more::{From, Into};

/// Import(s) from the guest.
///
/// See module header for more info, pls and thx.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportMap {
    /// Imported **functions**: each slot and the value in it.
    ///
    /// From the module header's example:
    ///
    /// ```text
    /// ThunkAddress(0x00429AE4) -> ImportToken(0x80000115)
    /// ```
    addr_index: HashMap<ThunkAddress, ImportToken>,

    /// Imported **data**.
    ///
    /// On Xbox, this is the 'XboxSignatureKey', and the slots are 4 bytes.
    /// But on other platforms can be *any size*.
    ///
    /// ```text
    /// { ThunkAddress(0x81FC2000) }
    /// ```
    data_slots: HashSet<ThunkAddress>,
}

/// Token that is replaced with actual address during xbe/exe load time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, From, Into)]
pub struct ImportToken(pub u32);

/// A guest address of a thunk-table item, e.g. `0x00429AE4` above.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, From, Into)]
pub struct ThunkAddress(pub u32);

impl ImportMap {
    /// Gets the import token for a thunk-table slot.
    ///
    /// # Arguments
    ///
    /// * `call_addr` - the thunk-table slot address to look up
    ///
    /// # Returns
    ///
    /// The token stored for `call_addr`, or [`None`].
    pub fn resolve_thunk(&self, call_addr: ThunkAddress) -> Option<ImportToken> {
        self.addr_index.get(&call_addr).copied()
    }

    /// Returns `true` if `address` is a data import.
    ///
    /// # Arguments
    ///
    /// * `address` - the address to test
    ///
    /// # Remarks
    ///
    /// If not data import, assume it's a code import, kthx
    pub fn is_data_import(&self, address: ThunkAddress) -> bool {
        self.data_slots.contains(&address)
    }

    /// Adds `address` as a slot holding imported data.
    ///
    /// # Arguments
    ///
    /// * `address` - the data import slot address
    pub fn add_data_import(&mut self, address: ThunkAddress) {
        self.data_slots.insert(address);
    }

    /// Adds a thunk address; and the import token for that slot.
    ///
    /// # Arguments
    ///
    /// * `address` - the thunk-table slot address
    /// * `token` - the import token found at that slot
    ///
    /// # Remarks
    ///
    /// First add wins, next ones don't override.
    pub fn add_thunk(&mut self, address: ThunkAddress, token: ImportToken) {
        self.addr_index.entry(address).or_insert(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Core behaviour ───────────────────────────────────────────────────────

    /// The first token added for an address wins.
    /// Same address again keeps the original token.
    #[test]
    fn add_thunk_keeps_the_first_address() {
        let mut imports = ImportMap::default();

        imports.add_thunk(ThunkAddress(0x8000_1000), ImportToken(0x42));
        imports.add_thunk(ThunkAddress(0x8000_1000), ImportToken(0x43));

        assert_eq!(
            imports.resolve_thunk(ThunkAddress(0x8000_1000)),
            Some(ImportToken(0x42))
        );
    }
}
