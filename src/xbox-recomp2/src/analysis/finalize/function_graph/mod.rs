//! Track which functions call, jump to, or take the address of other functions.

use crate::analysis::graph::{DiscoveryGraph, FunctionDiscoverySource};
pub use function_reference::{FunctionReference, ReferenceKind};
use std::collections::{BTreeMap, BTreeSet};
use xbox_recomp2_executable::GuestAddress32;

mod function_reference;

/// Every reference between functions that discovery found.
///
/// Use [`Self::outgoing`] to see what a function points to, and
/// [`Self::incoming`] to see what points to it.
///
/// # Reference kinds
///
/// - `call 0x2000` is a [`ReferenceKind::DirectCall`].
/// - A jump/branch/switch case going to another func is
///   [`ReferenceKind::TailCall`].
/// - If address is referenced in another instruction like `push 0x2000`,
///   then it shows up as [`ReferenceKind::PossiblePointer`].
#[derive(Debug, Default)]
pub struct FunctionGraph {
    outgoing: BTreeMap<GuestAddress32, Vec<FunctionReference>>,
    incoming: BTreeMap<GuestAddress32, Vec<FunctionReference>>,
    unresolved: BTreeMap<GuestAddress32, Vec<GuestAddress32>>,
    sources: BTreeMap<GuestAddress32, BTreeSet<FunctionDiscoverySource>>,
}

impl FunctionGraph {
    /// Builds reference lookups and takes ownership of discovery reasons.
    pub(in crate::analysis) fn from_discovery(graph: DiscoveryGraph) -> Self {
        // Get all references from all functions.
        let references = graph.functions.iter().flat_map(|(&source, function)| {
            function
                .references
                .iter()
                .map(move |reference| FunctionReference {
                    source,
                    site: reference.site,
                    target: reference.target,
                    kind: reference.kind.into(),
                })
        });

        // Construct the new function graph from these references and funcs.
        let mut references = Self::new(graph.functions.keys().copied(), references);

        // And also add the targets we don't know, like `jmp eax`.
        // We could PGO that later (if time allows); etc.
        for (entry, function) in graph.functions {
            references.sources.insert(entry, function.discovery_sources);
            references.add_unresolved(
                entry,
                function.unresolved.iter().map(|transfer| transfer.site),
            );
        }

        references
    }

    /// Given entries and references, create the function graph,
    /// with lookups (xrefs) in both directions.
    ///
    /// Each xref list is sorted with duplicates removed.
    ///
    /// # Arguments
    ///
    /// - `function_entries`: entry addresses of all known functions.
    /// - `function_references`: calls, tail calls, and address references.
    fn new(
        function_entries: impl IntoIterator<Item = GuestAddress32>,
        function_references: impl IntoIterator<Item = FunctionReference>,
    ) -> Self {
        let mut graph = Self::default();

        // Initialise entries for all functions.
        // This ensures we have empty entries.
        for entry in function_entries {
            graph.outgoing.entry(entry).or_default();
            graph.incoming.entry(entry).or_default();
        }

        // Fill in the function references.
        for reference in function_references {
            graph
                .outgoing
                .entry(reference.source)
                .or_default()
                .push(reference);
            graph
                .incoming
                .entry(reference.target)
                .or_default()
                .push(reference);
        }

        // Sort references and remove duplicates in both directions.
        for references in graph.outgoing.values_mut() {
            references.sort_unstable();
            references.dedup();
            references.shrink_to_fit();
        }

        for references in graph.incoming.values_mut() {
            references.sort_unstable();
            references.dedup();
            references.shrink_to_fit();
        }
        graph
    }

    /// Lists every function that discovery found.
    ///
    /// # Returns
    ///
    /// Function entry addresses, sorted in ascending order.
    pub fn functions(&self) -> impl Iterator<Item = GuestAddress32> + '_ {
        self.outgoing.keys().copied()
    }

    /// Lists the references made by instructions inside `function`.
    ///
    /// # Arguments
    ///
    /// - `function`: entry address of the function to look in.
    ///
    /// # Returns
    ///
    /// The references, or an empty slice if `function` isn't a known entry.
    pub fn outgoing(&self, function: GuestAddress32) -> &[FunctionReference] {
        self.outgoing.get(&function).map_or(&[], Vec::as_slice)
    }

    /// Lists the references that point to `function`.
    ///
    /// # Arguments
    ///
    /// - `function`: entry address of the function you want references to.
    ///
    /// # Returns
    ///
    /// The references, or an empty slice if `function` isn't a known entry.
    ///
    /// # Remarks
    ///
    /// This includes [`ReferenceKind::PossiblePointer`] references, which aren't
    /// calls. Skip those if you only want callers.
    ///
    /// This list may be missing callers. Before you change the function's
    /// calling convention, account for every path into it. If you can't, keep
    /// an entry that still uses the old convention.
    pub fn incoming(&self, function: GuestAddress32) -> &[FunctionReference] {
        self.incoming.get(&function).map_or(&[], Vec::as_slice)
    }

    /// Adds all functions whose targets you can't (really) figure out.
    /// Info `self.unresolved`.
    fn add_unresolved(
        &mut self,
        function: GuestAddress32,
        sites: impl Iterator<Item = GuestAddress32>,
    ) {
        let mut sites: Vec<_> = sites.collect();
        sites.sort_unstable();
        sites.dedup();
        if !sites.is_empty() {
            sites.shrink_to_fit();
            self.unresolved.insert(function, sites);
        }
    }

    /// Lists all ways we found the function entry. Including known and brute
    /// force strategies.
    pub(crate) fn discovery_sources(
        &self,
        function: GuestAddress32,
    ) -> Option<&BTreeSet<FunctionDiscoverySource>> {
        self.sources.get(&function)
    }

    /// Lists instructions in `function` whose targets we couldn't fully work
    /// out, such as `jmp eax`.
    ///
    /// # Arguments
    ///
    /// - `function`: entry address of the function to look in.
    ///
    /// # Returns
    ///
    /// Instruction addresses, lowest first. The slice is empty if `function`
    /// has no such instructions or isn't a known entry.
    ///
    /// # Remarks
    ///
    /// We may still have found some of their targets. Those appear in
    /// [`Self::outgoing`], but there may be more.
    pub fn unresolved_sites(&self, function: GuestAddress32) -> &[GuestAddress32] {
        self.unresolved.get(&function).map_or(&[], Vec::as_slice)
    }
}
