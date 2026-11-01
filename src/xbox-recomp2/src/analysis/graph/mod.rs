//! Tracks discovered functions and the instructions each one can reach.
//!
//! # Layout
//!
//! - [`DiscoveryGraph`]: one [`FunctionNode`] per discovered function entry.
//! - [`BasicBlock`]: a straight-line run of instructions in one function.
//! - [`LocalEdge`]: a path between instructions in the same function.
//! - [`FunctionReference`]: a link from an instruction to another function.
//!
//! # Example
//!
//! Function A starts at `0x1000`:
//!
//! ```asm
//! 0x1000: call 0x2000
//! 0x1005: test eax, eax
//! 0x1007: jnz 0x1010
//! 0x1009: ret
//! 0x1010: ret
//! ```
//!
//! - The `call` adds a [`FunctionReference`] to `0x2000`.
//!   That address gets its own [`FunctionNode`].
//! - The `jnz` adds two [`LocalEdge`]s inside A.
//!   One goes to `0x1010` (taken), the other to `0x1009` (not taken).
//!
//! See "How to read the discovery graph" in `doc/docs/function-discovery.md`
//! for diagrams of these cases.

use crate::config::FunctionSize;
pub(super) use basic_block::{BasicBlock, EdgeKind, LocalEdge};
pub(crate) use function_node::{BruteForceKind, FunctionDiscoverySource};
pub(super) use function_node::{Diagnostic, FunctionNode, LearnedTargets, StopReason};
pub(crate) use function_reference::ReferenceKind;
pub(super) use function_reference::{FunctionReference, TransferKind, UnresolvedTransfer};
pub(super) use instruction::{Flow, Instruction, MemoryOperand, Operand, OperandKind};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

mod basic_block;
mod function_node;
mod function_reference;
mod instruction;
#[cfg(test)]
pub(super) mod test_helpers;

/// Every function found so far.
#[derive(Debug, Default)]
pub(super) struct DiscoveryGraph {
    /// The discovered functions.
    pub functions: BTreeMap<GuestAddress32, FunctionNode>,
}

impl DiscoveryGraph {
    /// Adds a function entry, or adds another source to an existing one.
    ///
    /// # Arguments
    ///
    /// - `entry`: guest address where the function starts.
    /// - `source`: how this entry was found.
    /// - `size`: byte size, if known. [`None`] keeps any size already set.
    ///
    /// # Returns
    ///
    /// `true` if the entry is new, or its sources or size changed.
    /// Otherwise `false`.
    pub fn add_function(
        &mut self,
        entry: GuestAddress32,
        source: FunctionDiscoverySource,
        size: Option<FunctionSize>,
    ) -> bool {
        let is_new = !self.functions.contains_key(&entry);
        let node = self
            .functions
            .entry(entry)
            .or_insert_with(|| FunctionNode::new(entry));
        let mut changed = node.discovery_sources.insert(source) || is_new;
        if let Some(size) = size {
            changed |= node.authoritative_size != Some(size);
            node.authoritative_size = Some(size);
        }

        changed
    }

    /// Stores a reference to another function and adds the target to the graph.
    ///
    /// # Arguments
    ///
    /// - `function`: entry address of the function containing the reference.
    /// - `reference`: instruction address, target entry, and reference kind.
    ///
    /// # Returns
    ///
    /// `true` if the reference is new or the target's sources changed.
    /// `false` if `function` isn't in the graph or nothing changed.
    pub fn add_reference(
        &mut self,
        function: GuestAddress32,
        reference: FunctionReference,
    ) -> bool {
        let Some(node) = self.functions.get_mut(&function) else {
            return false;
        };
        let changed = node.learned.references.insert(reference);
        node.references.insert(reference);
        let seeded = self.add_function(
            reference.target,
            FunctionDiscoverySource::Reference {
                function,
                site: reference.site,
                kind: reference.kind,
            },
            None,
        );

        changed || seeded
    }
}

#[cfg(test)]
mod tests {
    //! yay ✨
    use super::*;
    use rstest::rstest;

    /// Adding the same entry twice keeps the first source and size.
    #[rstest]
    #[case::entry(FunctionDiscoverySource::EntryPoint, None)]
    #[case::manual(FunctionDiscoverySource::Manual, Some(FunctionSize(16)))]
    #[case::fallback(FunctionDiscoverySource::BruteForce { kind: BruteForceKind::Padding }, None)]
    fn add_function_should_keep_first_entry_and_source(
        #[case] source: FunctionDiscoverySource,
        #[case] size: Option<FunctionSize>,
    ) {
        // Arrange.
        let mut graph = DiscoveryGraph::default();
        let entry = GuestAddress32::from(0x1000);

        // Act.
        let first = graph.add_function(entry, source, size);
        let duplicate = graph.add_function(entry, source, None);

        // Assert.
        assert!(first);
        assert!(!duplicate);
        assert_eq!(graph.functions[&entry].authoritative_size, size);
        assert_eq!(graph.functions[&entry].discovery_sources, [source].into());
    }
}
