//! Schedule function walks when new entries, configured edges, or pointers change.

use super::super::graph::{DiscoveryGraph, FunctionNode};
use address_owners::AddressOwners;
use std::collections::{BTreeMap, BTreeSet};
use xbox_recomp2_executable::GuestAddress32;

mod address_owners;

/// Functions waiting for a walk.
/// With lookups to find which functions need a retry.
///
/// # Remarks
///
/// Several functions can cover the same bytes.
/// Track each one so a change queues all affected functions.
pub(super) struct WorkQueue {
    /// Functions waiting for a walk, keyed by entry address.
    ///
    /// If true, scan pointers and apply configured edges after any stable walk.
    /// If false, do so only when the walk changes the body.
    pending: BTreeMap<GuestAddress32, bool>,

    tracked: BTreeSet<GuestAddress32>,

    /// Decoded bytes -> function entries whose last walk covered those bytes.
    bodies: AddressOwners,

    /// Jump targets -> functions whose local edges could become tail calls.
    jumps: BTreeMap<GuestAddress32, BTreeSet<GuestAddress32>>,
}

impl WorkQueue {
    /// Queues every known function for a walk.
    ///
    /// # Arguments
    /// - `graph`: Discovery graph containing the functions to visit.
    ///
    /// # Returns
    /// Queue for function walks and post-walk pointer scans.
    pub fn new(graph: &DiscoveryGraph) -> Self {
        Self {
            pending: graph.functions.keys().map(|&entry| (entry, true)).collect(),
            tracked: BTreeSet::new(),
            bodies: AddressOwners::default(),
            jumps: BTreeMap::new(),
        }
    }

    /// Dequeues the function with the lowest entry address.
    ///
    /// # Returns
    /// `Some((entry, force_pointer_scan))` for the next function,
    /// or [`None`] when no functions are pending.
    pub fn pop(&mut self) -> Option<(GuestAddress32, bool)> {
        self.pending.pop_first()
    }

    /// Queues another walk for a function after an edge or reference changes.
    ///
    /// # Arguments
    /// - `entry`: Function entry whose edges or references changed.
    pub fn rewalk(&mut self, entry: GuestAddress32) {
        self.pending.entry(entry).or_insert(false);
    }

    /// Checks whether a function needs another walk before we can explore deeper.
    ///
    /// # Arguments
    /// - `entry`: Entry of the function just walked.
    ///
    /// # Returns
    /// - `true`: The queue holds another request for `entry` with a forced pointer scan.
    /// - `false`: No such request exists. A retry from [`Self::rewalk`] alone
    ///   returns `false`.
    pub fn is_invalidated(&self, entry: GuestAddress32) -> bool {
        self.pending.get(&entry) == Some(&true)
    }

    /// Queues new functions and rechecks existing functions they affect.
    ///
    /// In rare-ish cases, a function can start within bytes decoded for
    /// another function. For example a recursive function that jumps
    /// into the middle of itself.
    ///
    /// In such a case, we need to reconsider that function's boundaries.
    ///
    /// We also recheck functions that jump to the new entry, because those
    /// jumps may be tail calls.
    ///
    /// # Arguments
    /// - `entries`: New function entries already added to the discovery graph.
    pub fn learn_entries(&mut self, entries: impl IntoIterator<Item = GuestAddress32>) {
        for entry in entries {
            self.pending.insert(entry, true);
            self.pending
                .extend(self.bodies.at(entry).map(|owner| (owner, true)));

            // Jumps to this entry can become tail calls.
            if let Some(owners) = self.jumps.get(&entry) {
                self.pending
                    .extend(owners.iter().map(|&owner| (owner, true)));
            }
        }
    }

    /// Saves a walked body's byte ranges and jump targets for later retries.
    ///
    /// # Arguments
    /// - `function`: Function node from the completed walk.
    ///
    /// # Remarks
    ///
    /// Before replacing a body, call [`Self::untrack_dependencies`] with the old
    /// node. After walking it again, call this method with the rebuilt node.
    pub fn track_dependencies(&mut self, function: &FunctionNode) {
        self.tracked.insert(function.entry);

        // In case of gaps, e.g.
        body_ranges(function, |start, end| {
            self.bodies.insert(start, end, function.entry)
        });
        for target in jump_targets(function) {
            self.jumps.entry(target).or_default().insert(function.entry);
        }
    }

    /// Stops tracking a body's byte ranges and jump targets before another walk.
    ///
    /// # Arguments
    /// - `function`: Node matching the tracked byte ranges and jump targets.
    ///
    /// # Remarks
    ///
    /// Other functions that cover the same bytes or share jump targets remain tracked.
    pub fn untrack_dependencies(&mut self, function: &FunctionNode) {
        // Yeet out the node.
        if !self.tracked.remove(&function.entry) {
            return;
        }

        // And remove its tracked byte ranges and jump targets.
        body_ranges(function, |start, end| {
            self.bodies.remove(start, end, function.entry)
        });
        for target in jump_targets(function) {
            if let Some(owners) = self.jumps.get_mut(&target) {
                owners.remove(&function.entry);
                if owners.is_empty() {
                    self.jumps.remove(&target);
                }
            }
        }
    }
}

// Visit the decoded instructions as continuous byte ranges:
// - Each range runs from start up to, but not including, end.
// - Skip gaps between instructions.
// - The end address is a u64.
fn body_ranges(function: &FunctionNode, mut visit: impl FnMut(GuestAddress32, u64)) {
    let mut instructions = function.instructions.values();
    let Some(first) = instructions.next() else {
        return;
    };

    let mut range_start = first.address;
    let mut range_end = first.end();

    // Iterate the instructions
    for instruction in instructions {
        let address = u64::from(instruction.address.get());

        // If this instruction doesn't follow the previous one, start a new range.
        if address != range_end {
            visit(range_start, range_end);
            range_start = instruction.address;
        }

        range_end = instruction.end();
    }

    // Visit the last range.
    visit(range_start, range_end);
}

// Return jump targets that could become tail calls.
// If a target becomes a known function entry, revisit this function.
fn jump_targets(function: &FunctionNode) -> impl Iterator<Item = GuestAddress32> + '_ {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.successors)
        .filter(|edge| edge.kind.can_tail_call())
        .map(|edge| edge.target)
}

// TODO: Add tests here. Am lacking in time.
