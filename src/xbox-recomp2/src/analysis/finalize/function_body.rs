//! Which instructions belong to a function, and how execution moves between them.
//!
//! # Usage
//!
//! After analysis, get a function's body from [`Recompiler::function_body`],
//! or build one with [`FunctionBody::new`]. Then query it:
//!
//! - [`FunctionBody::entry`]: where execution starts.
//! - [`FunctionBody::contains`]: whether an instruction starts at an address.
//! - [`FunctionBody::starts`] and [`FunctionBody::lengths`]: each instruction's
//!   address and size, in address order.
//! - [`FunctionBody::edges`]: the paths between instructions.
//!
//! Example: a `jmp` that skips over data.
//!
//! ```text
//! 0x1000: jmp 0x1010  ; 2 bytes
//! 0x1002: (data)      ; not an instruction, so not in the body
//! 0x1010: ret         ; 1 byte
//! ```
//!
//! ```rust
//! use xbox_recomp2::analysis::{BodyEdge, EdgeKind, FunctionBody};
//! use xbox_recomp2_executable::GuestAddress32;
//!
//! let body = FunctionBody::new(
//!     0x1000u32.into(),
//!     [(GuestAddress32::from(0x1000), 2), (GuestAddress32::from(0x1010), 1)],
//!     [BodyEdge {
//!         source: 0x1000u32.into(),
//!         target: 0x1010u32.into(),
//!         kind: EdgeKind::Jump,
//!     }],
//! );
//!
//! assert_eq!(body.entry().get(), 0x1000);
//! assert!(body.contains(0x1010u32.into()));
//! assert!(!body.contains(0x1004u32.into())); // Inside the data.
//! assert_eq!(body.next_start_after(0x1000u32.into()).map(|address| address.get()), Some(0x1010));
//! ```
//!
//! [`Recompiler::function_body`]: crate::Recompiler::function_body

use crate::analysis::graph::EdgeKind as DiscoveryEdgeKind;
use std::collections::BTreeSet;
use xbox_recomp2_executable::GuestAddress32;

/// The instructions in one function, and the paths between them.
///
/// # Remarks
///
/// Stored instructions are sorted in address order, i.e. 0x1000, 0x1002,
/// 0x1004, ...
///
/// We will generate them in the same linear order down the road.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionBody {
    /// Address where execution starts.
    entry: GuestAddress32,

    /// Address of each instruction, sorted, no duplicates.
    starts: Box<[GuestAddress32]>,

    /// Size in bytes of each instruction, in the same order as `starts`.
    lengths: Box<[u8]>,

    /// Paths between instructions, sorted.
    edges: Box<[BodyEdge]>,
}

/// The instruction addresses in one [`FunctionBody`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalTargets(Box<[GuestAddress32]>);

/// A path between instructions in the same function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyEdge {
    /// Address of the instruction the path starts at.
    pub source: GuestAddress32,

    /// Address of the instruction the path leads to.
    pub target: GuestAddress32,

    /// How execution gets from `source` to `target`.
    pub kind: EdgeKind,
}

/// How execution gets from one instruction to another in the same function.
///
/// This branch gives `0x1007` two local paths:
///
/// ```asm
/// 0x1007: jnz 0x1010  ; BranchTaken -> 0x1010
///                     ; BranchNotTaken -> 0x1009
/// 0x1009: ret
/// 0x1010: ret
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    /// Continue at the next instruction after ordinary code or a returning
    /// call.
    ///
    /// ```asm
    /// 0x1000: mov eax, ebx  ; NextInstruction -> 0x1002
    /// 0x1002: call 0x2000   ; NextInstruction -> 0x1007 when the call returns
    /// 0x1007: ret
    /// ```
    NextInstruction,

    /// Follow the target of `jnz 0x1010` when the condition holds.
    BranchTaken,

    /// Continue after `jnz 0x1010` when the condition does not hold.
    BranchNotTaken,

    /// Follow a jump inside this function, such as `jmp 0x1010`.
    ///
    /// # Remarks
    ///
    /// Also used for mid-assembly hook jumps, and for the branch at a hooked
    /// instruction.
    Jump,

    /// Follow a case address read from a table by `jmp [eax*4+0x4000]`.
    SwitchCase,
}

impl FunctionBody {
    /// Builds a [`FunctionBody`].
    ///
    /// - Sorts instructions by address. If an address appears twice, keeps
    ///   the smaller length.
    /// - Sorts edges by `(source, target, kind)`. Drops edges that start or
    ///   end outside the given instructions.
    ///
    /// # Arguments
    ///
    /// - `entry`: where execution starts.
    /// - `instructions`: `(address, size in bytes)` for each instruction, in
    ///   any order.
    /// - `edges`: the paths between instructions.
    pub fn new<I, E>(entry: GuestAddress32, instructions: I, edges: E) -> Self
    where
        I: IntoIterator<Item = (GuestAddress32, u8)>,
        E: IntoIterator<Item = BodyEdge>,
    {
        let mut instructions: Vec<(GuestAddress32, u8)> = instructions.into_iter().collect();

        // Sort by address.
        // We generate code in linear order down the road, so need sorting.
        // Thanks mate!
        instructions.sort_unstable();
        instructions.dedup_by_key(|(address, _)| *address);

        let selected: BTreeSet<GuestAddress32> =
            instructions.iter().map(|&(address, _)| address).collect();

        let mut edges: Vec<BodyEdge> = edges
            .into_iter()
            .filter(|edge| selected.contains(&edge.source) && selected.contains(&edge.target))
            .collect();
        edges.sort_unstable();
        edges.dedup();

        let (starts, lengths): (Vec<GuestAddress32>, Vec<u8>) = instructions.into_iter().unzip();

        Self {
            entry,
            starts: starts.into_boxed_slice(),
            lengths: lengths.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
        }
    }

    /// Returns the address execution starts at.
    pub fn entry(&self) -> GuestAddress32 {
        self.entry
    }

    /// Returns the address of each instruction, in address order.
    pub fn starts(&self) -> &[GuestAddress32] {
        &self.starts
    }

    /// Returns the size in bytes of each instruction, in the same order as
    /// [`FunctionBody::starts`].
    pub fn lengths(&self) -> &[u8] {
        &self.lengths
    }

    /// Returns the paths between instructions, sorted by `(source, target, kind)`.
    pub fn edges(&self) -> &[BodyEdge] {
        &self.edges
    }

    /// Returns the lowest instruction address, or [`None`] for an empty body.
    pub fn first_address(&self) -> Option<GuestAddress32> {
        self.starts.first().copied()
    }

    /// Returns the address of the instruction written after `address`, or
    /// [`None`] after the last one.
    ///
    /// ```text
    /// Instructions: 0x1000, 0x1002, 0x1010
    ///
    /// next_start_after(0x1000) == Some(0x1002)
    /// next_start_after(0x1002) == Some(0x1010)  // skips the gap
    /// next_start_after(0x1010) == None
    /// ```
    pub fn next_start_after(&self, address: GuestAddress32) -> Option<GuestAddress32> {
        let index = self.starts.partition_point(|&start| start <= address);
        self.starts.get(index).copied()
    }

    /// Returns whether an instruction in this body starts at `address`.
    ///
    /// ```text
    /// Instructions: 0x1000 (2 bytes), 0x1010
    ///
    /// contains(0x1000) == true
    /// contains(0x1001) == false  // middle of an instruction
    /// contains(0x1004) == false  // data between instructions
    /// ```
    ///
    /// # Arguments
    ///
    /// - `address`: the guest address to look up.
    ///
    /// # Returns
    ///
    /// `true` if an instruction starts exactly at `address`.
    pub fn contains(&self, address: GuestAddress32) -> bool {
        self.starts.binary_search(&address).is_ok()
    }

    /// Returns the instruction addresses as [`LocalTargets`].
    pub fn local_targets(&self) -> LocalTargets {
        LocalTargets::new(self.starts.iter().copied())
    }
}

impl LocalTargets {
    /// Builds the set from `addresses` in any order, dropping duplicates.
    pub fn new<I>(addresses: I) -> Self
    where
        I: IntoIterator<Item = GuestAddress32>,
    {
        let mut sorted: Vec<GuestAddress32> = addresses.into_iter().collect();
        sorted.sort_unstable();
        sorted.dedup();
        Self(sorted.into_boxed_slice())
    }

    /// Returns whether `address` is in the set.
    pub fn contains(&self, address: GuestAddress32) -> bool {
        self.0.binary_search(&address).is_ok()
    }
}

impl EdgeKind {
    /// Returns whether execution resumes with next instruction.
    /// Basically true if we're not calling or doing a branch.
    ///
    /// This kinda tells us where/when we need a label.
    pub fn is_sequential(self) -> bool {
        matches!(self, EdgeKind::NextInstruction | EdgeKind::BranchNotTaken)
    }
}

impl From<DiscoveryEdgeKind> for EdgeKind {
    fn from(kind: DiscoveryEdgeKind) -> Self {
        match kind {
            DiscoveryEdgeKind::NextInstruction => Self::NextInstruction,
            DiscoveryEdgeKind::BranchTaken => Self::BranchTaken,
            DiscoveryEdgeKind::BranchNotTaken => Self::BranchNotTaken,
            DiscoveryEdgeKind::Jump => Self::Jump,
            DiscoveryEdgeKind::SwitchCase => Self::SwitchCase,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for building and querying function bodies.

    use super::{BodyEdge, EdgeKind, FunctionBody, LocalTargets};
    use xbox_recomp2_executable::GuestAddress32;

    // ── Construction ─────────────────────────────────────────────────────────

    /// Instructions sort by address, and duped address keeps its shorter length.
    #[test]
    fn new_should_sort_instructions_and_keep_the_shorter_repeat() {
        // 0x1000: 31 C0           xor eax, eax
        // 0x1002: B8 01 00 00 00  mov eax, 1
        let body = body_with(0x1000, [(0x1002, 5), (0x1000, 3), (0x1000, 2)], []);

        assert_eq!(body.starts(), [0x1000, 0x1002].map(GuestAddress32::from));
        assert_eq!(body.lengths(), &[2, 5]);
    }

    /// Edges sort by source, target, then kind.
    /// Edges that start/end outside the body are dropped.
    #[test]
    fn new_should_sort_edges_and_drop_repeats_and_outside_edges() {
        // 0x1000: 31 C0  xor eax, eax
        // 0x1002: 75 FC  jnz 0x1000
        // 0x1004: C3     ret
        let next = edge(0x1000, 0x1002, EdgeKind::NextInstruction);
        let taken = edge(0x1002, 0x1000, EdgeKind::BranchTaken);
        let not_taken = edge(0x1002, 0x1004, EdgeKind::BranchNotTaken);
        let body = body_with(
            0x1000,
            [(0x1000, 2), (0x1002, 2), (0x1004, 1)],
            [
                not_taken,
                edge(0x9000, 0x1000, EdgeKind::Jump), // Starts outside.
                taken,
                next,
                taken,
                edge(0x1000, 0x7FFF, EdgeKind::Jump), // Ends outside.
            ],
        );

        assert_eq!(body.edges(), &[next, taken, not_taken]);
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    /// The queries find instructions and skip the gap between them. The entry
    /// and the first address can differ.
    #[test]
    fn queries_should_skip_the_gap_between_instructions() {
        // 0x1000: 31 C0  xor eax, eax
        // 0x1002: 90     nop           ; entry
        // 0x1003: (data)               ; gap
        // 0x1006: C3     ret
        let body = body_with(0x1002, [(0x1000, 2), (0x1002, 1), (0x1006, 1)], []);

        // The entry is 0x1002, but the lowest instruction is 0x1000.
        assert_eq!(body.entry().get(), 0x1002);
        assert_eq!(
            body.first_address().map(|address| address.get()),
            Some(0x1000)
        );

        // 0x1006 starts an instruction.
        assert!(body.contains(GuestAddress32::from(0x1006)));

        // 0x1004 is in the gap.
        assert!(!body.contains(GuestAddress32::from(0x1004)));

        // After the entry comes 0x1006.
        assert_eq!(
            body.next_start_after(GuestAddress32::from(0x1002))
                .map(|address| address.get()),
            Some(0x1006)
        );

        // Nothing follows 0x1006.
        assert_eq!(body.next_start_after(GuestAddress32::from(0x1006)), None);

        // Shuffled and repeated input still gives each address once.
        assert_eq!(
            body.local_targets(),
            LocalTargets::new([0x1006, 0x1000, 0x1002, 0x1000].map(GuestAddress32::from))
        );
    }

    /// Builds an edge from plain [`u32`] addresses.
    fn edge(source: u32, target: u32, kind: EdgeKind) -> BodyEdge {
        BodyEdge {
            source: source.into(),
            target: target.into(),
            kind,
        }
    }

    /// Builds a body from plain [`u32`] addresses.
    fn body_with<I>(
        entry: u32,
        instructions: I,
        edges: impl IntoIterator<Item = BodyEdge>,
    ) -> FunctionBody
    where
        I: IntoIterator<Item = (u32, u8)>,
    {
        FunctionBody::new(
            entry.into(),
            instructions
                .into_iter()
                .map(|(address, length)| (address.into(), length)),
            edges,
        )
    }
}
