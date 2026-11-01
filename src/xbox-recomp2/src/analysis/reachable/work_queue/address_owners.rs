//! Track which functions cover each decoded byte range.

use std::collections::{BTreeMap, BTreeSet};
use xbox_recomp2_executable::{GuestAddress32, GuestAddress64};

/// Maps decoded byte ranges to the function entries that own them.
///
/// Each map key marks the start of an owner set. That set applies up to, but
/// not including, the next key. Bytes before the first key have no owners.
/// This stores boundaries rather than a separate owner for every instruction.
///
/// For example, adding A over `0x1000..0x1004` to an empty map gives:
///
/// ```text
/// Boundary   Owners   Covered bytes
/// 0x1000     {A}      0x1000..0x1004
/// 0x1004     {}       0x1004 onward
/// ```
///
/// Now add B over `0x1002..0x1006`. Split A's range at `0x1002` so the
/// overlapping bytes can belong to both functions:
///
/// ```text
/// Boundary   Owners   Covered bytes
/// 0x1000     {A}      0x1000..0x1002
/// 0x1002     {A, B}   0x1002..0x1004
/// 0x1004     {B}      0x1004..0x1006
/// 0x1006     {}       0x1006 onward
/// ```
///
/// Removing A from its range leaves B covering `0x1002..0x1006`.
/// Adding or removing one owner does not change the other owners.
#[derive(Default)]
pub(super) struct AddressOwners {
    /// Owner sets keyed by the guest address where each set begins.
    ///
    /// Keys use 64 bits so an exclusive end can extend past the last 32-bit byte.
    boundaries: BTreeMap<GuestAddress64, BTreeSet<GuestAddress32>>,
}

impl AddressOwners {
    /// Looks up functions covering a decoded byte.
    ///
    /// # Arguments
    /// - `address`: Guest byte address to look up.
    ///
    /// # Returns
    /// Function entries covering `address`, or an empty iterator for a gap.
    pub fn at(&self, address: GuestAddress32) -> impl Iterator<Item = GuestAddress32> + '_ {
        // The last boundary at or before the address supplies its owner set.
        self.boundaries
            .range(..=GuestAddress64::from(address))
            .next_back()
            .into_iter()
            .flat_map(|(_, owners)| owners.iter().copied())
    }

    /// Assigns `owner` to the guest byte range `start..end`.
    ///
    /// # Arguments
    /// - `start`: First guest byte in the range.
    /// - `end`: Exclusive end, without 32-bit wrapping.
    /// - `owner`: Function entry to add.
    pub fn insert(&mut self, start: GuestAddress32, end: u64, owner: GuestAddress32) {
        self.update(start.into(), end.into(), owner, true);
    }

    /// Removes `owner` from the guest byte range `start..end`.
    ///
    /// # Arguments
    /// - `start`: First guest byte in the range.
    /// - `end`: Exclusive end, without 32-bit wrapping.
    /// - `owner`: Function entry to remove; other owners remain unchanged.
    pub fn remove(&mut self, start: GuestAddress32, end: u64, owner: GuestAddress32) {
        self.update(start.into(), end.into(), owner, false);
    }

    fn update(
        &mut self,
        start: GuestAddress64,
        end: GuestAddress64,
        owner: GuestAddress32,
        insert: bool,
    ) {
        // Put boundaries at both ends without changing their owner sets.
        // This lets us edit start..end without affecting bytes outside it.
        self.split(start);
        self.split(end);

        // The range may contain several owner sets. Update each one, but leave
        // the set at the exclusive end unchanged.
        for owners in self
            .boundaries
            .range_mut(start..end)
            .map(|(_, owners)| owners)
        {
            if insert {
                owners.insert(owner);
            } else {
                owners.remove(&owner);
            }
        }

        // Drop either endpoint if it no longer separates different owner sets.
        self.remove_redundant_boundary(start);
        self.remove_redundant_boundary(end);
    }

    // Find the owner set immediately before the address, excluding a boundary there.
    fn owners_before(&self, address: GuestAddress64) -> Option<&BTreeSet<GuestAddress32>> {
        self.boundaries
            .range(..address)
            .next_back()
            .map(|(_, owners)| owners)
    }

    // Add a boundary without changing ownership. For split(0x1002):
    //
    // Before: 0x1000 -> {A},                 0x1004 -> {}
    // After:  0x1000 -> {A}, 0x1002 -> {A},  0x1004 -> {}
    fn split(&mut self, address: GuestAddress64) {
        // Keep an existing boundary. A new one inherits the preceding owners,
        // or an empty set if there is no earlier boundary.
        if !self.boundaries.contains_key(&address) {
            let owners = self.owners_before(address).cloned().unwrap_or_default();
            self.boundaries.insert(address, owners);
        }
    }

    // Remove a boundary if its owners match the range just before it.
    // For remove_redundant_boundary(0x1002):
    //
    // Before: 0x1000 -> {A}, 0x1002 -> {A}, 0x1004 -> {}
    // After:  0x1000 -> {A},                0x1004 -> {}
    fn remove_redundant_boundary(&mut self, address: GuestAddress64) {
        // Before the first boundary, ownership is implicitly empty.
        let owners = &self.boundaries[&address];
        let redundant = self
            .owners_before(address)
            .map_or(owners.is_empty(), |previous| previous == owners);
        if redundant {
            self.boundaries.remove(&address);
        }
    }
}
