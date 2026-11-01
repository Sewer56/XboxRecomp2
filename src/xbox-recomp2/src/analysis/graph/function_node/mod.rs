//! Track what we know about each function as we explore the executable.
//!
//! Suppose the function at `0x1000` contains `call 0x2000`. The call tells us
//! about another function at `0x2000`, so we give that function its own node.
//!
//! See `doc/docs/function-discovery.md` for more info.

use super::{
    BasicBlock, FunctionReference, GuestAddress32, Instruction, ReferenceKind, UnresolvedTransfer,
};
use crate::config::FunctionSize;
pub(in crate::analysis) use learned_targets::LearnedTargets;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod learned_targets;

/// A function's starting address and the code reached from there.
///
/// # Remarks
///
/// Due to our brute force strategies, it's possible for two functions to
/// overlap in the executable; usually due to a misfire.
///
/// MISFIRING SYSTEM <https://youtu.be/40xqR0dSgL0>
///
/// In such a case, they have separate nodes. Ok? Baaaaaka >w<
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct FunctionNode {
    /// Guest entry address.
    pub entry: GuestAddress32,

    /// Why this entry was found. A call adds a
    /// [`FunctionDiscoverySource::Reference`] to its target.
    pub discovery_sources: BTreeSet<FunctionDiscoverySource>,

    /// Optional byte limit from `fixes.functions`.
    ///
    /// - [`None`]: no limit. Finalisation measures the decoded instructions.
    /// - [`Some`]: the walk stays inside `[entry, entry + size)`.
    ///
    /// A size of zero decodes nothing.
    pub authoritative_size: Option<FunctionSize>,

    /// Targets kept for later walks. The body below is rebuilt each time.
    pub learned: LearnedTargets,

    /// Instructions decoded from the original .xbe.
    pub instructions: BTreeMap<GuestAddress32, Arc<Instruction>>,

    /// Straight-line blocks in ascending start-address order.
    pub blocks: Vec<BasicBlock>,

    /// Reached calls or jumps whose destinations we cannot fully determine.
    pub unresolved: BTreeSet<UnresolvedTransfer>,

    /// Where the walker got stuck and why, such as unmapped bytes.
    pub diagnostics: BTreeSet<Diagnostic>,

    /// Other functions found during the walk.
    pub references: BTreeSet<FunctionReference>,
}

/// Where the walker stopped and what went wrong there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct Diagnostic {
    /// Address that could not be interpreted on this path.
    pub address: GuestAddress32,

    /// Why walking stopped at that address.
    pub reason: StopReason,
}

/// How we found a function entry.
///
/// One entry can be found in several ways.
/// This lists the ways it can be discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FunctionDiscoverySource {
    /// The executable's entry point, such as `_xstart`.
    EntryPoint,

    /// Provided in user configuration file.
    Manual,

    /// An existing function symbol in the executable's symbol table, at an
    /// address the walker can read as code.
    Symbol,

    /// A possible function start found by brute-force scans after the first walk:
    ///
    /// - Code references: calls, callback pointers, or function-pointer tables.
    /// - Padding: likely starts after padded instructions.
    /// - Tail jumps: jumps that leave a known candidate's interval.
    ///
    /// Each candidate gets its own walk. A scan match alone doesn't prove it's
    /// a function.
    BruteForce {
        /// What the scan found at this entry.
        kind: BruteForceKind,
    },

    /// Another function referred to this entry, for example by `call 0x2000`.
    Reference {
        /// Entry address of the function containing the reference.
        function: GuestAddress32,
        /// Address of the referring instruction, such as the `call`.
        site: GuestAddress32,
        /// Whether the instruction calls, jumps to, or takes the address of the entry.
        kind: ReferenceKind,
    },
}

/// What a brute force whole-section scan found for possible function entries.
///
/// Unlike walked [`ReferenceKind`] findings, the scanned bytes may be data or
/// unreachable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BruteForceKind {
    /// An aligned section start or a start after padding.
    Padding,

    /// A scanned `call 0x10000` at `site`.
    DirectCall {
        /// Address of the `call`.
        site: GuestAddress32,
    },

    /// A scanned jump at `site` that leaves its guessed function.
    TailCall {
        /// Address of the `jmp`.
        site: GuestAddress32,
    },

    /// A scanned instruction at `site` that may use the entry's address:
    ///
    /// ```asm
    /// push 0x10000      ; <- func address
    /// mov eax, 0x10000  ; <- func address
    /// ```
    PossiblePointer {
        /// Address of the referring instruction, or the table that holds it.
        site: GuestAddress32,
    },
}

/// Why the walker could not follow a path.
///
/// - `jmp eax` may lead somewhere unknown. We store it as an
///   [`UnresolvedTransfer`].
/// - Unmapped bytes add a [`Diagnostic`] with [`Self::Unmapped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) enum StopReason {
    /// No file bytes to decode here, or the address is past the executable's
    /// limit.
    ///
    /// For an XBE, this includes addresses outside its sections and the
    /// zero-filled tail of a code section.
    Unmapped,

    /// The address is in a section not classified as code, such as a data section.
    NotExecutable,

    /// The address is in a configured banned or invalid-instruction range.
    /// Sequential walking skips these ranges, but a jump into one stops here.
    Excluded,

    /// The address is outside this function's configured byte range, or the
    /// instruction would extend past the end of that range.
    OutsideFunction,

    /// The decoder rejects the bytes here. e.g. invalid instruction, cut short,
    /// etc.
    DecodeFailed,

    /// This path would decode bytes already used by another instruction in this
    /// function. i.e. `jmp` into middle of another instruction.
    OverlappingInstruction,

    /// The instruction decoded, but the walker can't follow what comes next.
    ///
    /// - `jmp far [eax]` changes the code segment; the walker cannot follow it.
    /// - `jmp eax` has an unknown target, but is an ordinary, supported jump.
    /// - Unsupported flow adds both a stop and an [`UnresolvedTransfer`].
    UnsupportedControlFlow,

    /// Continuing to the next instruction or past an excluded range would
    /// require an address beyond `0xFFFF_FFFF`, the last 32-bit guest address.
    AddressOverflow,
}

impl FunctionNode {
    /// Creates an empty body; graph insertion supplies its discovery source.
    pub(in crate::analysis) fn new(entry: GuestAddress32) -> Self {
        Self {
            entry,
            discovery_sources: BTreeSet::new(),
            authoritative_size: None,
            instructions: BTreeMap::new(),
            blocks: Vec::new(),
            references: BTreeSet::new(),
            unresolved: BTreeSet::new(),
            diagnostics: BTreeSet::new(),
            learned: LearnedTargets::default(),
        }
    }

    /// Finds a block by its first instruction in the address-sorted body.
    ///
    /// # Arguments
    ///
    /// - `start`: address of the block's first instruction. Later addresses in
    ///   the block don't match.
    ///
    /// # Returns
    ///
    /// The block, or `None` if `start` is not a block leader.
    #[cfg(test)]
    pub(in crate::analysis) fn block(&self, start: GuestAddress32) -> Option<&BasicBlock> {
        self.blocks
            .binary_search_by_key(&start, |block| block.start)
            .ok()
            .map(|index| &self.blocks[index])
    }

    /// Finds where the last decoded instruction ends.
    ///
    /// We use this for determining the function size.
    ///
    /// Generally, we treat functions as a straight line of instructions inside
    /// memory with the occasional tail call (which we already handle during
    /// walk).
    ///
    /// So therefore, we just pick highest instruction address from decoded.
    ///
    /// # Returns
    ///
    /// The exclusive end address as a [`u64`], or [`None`] if there are no instructions.
    pub fn decoded_end(&self) -> Option<u64> {
        self.instructions
            .last_key_value()
            .map(|(_, insn)| insn.end())
    }
}
