//! Track function references and unresolved transfers.
//!
//! - `call 0x2000` refers to another function.
//! - `jnz 0x1010` is a local path if its target is in this function.
//! - `jmp eax` remains unresolved even if one possible target is found.

use xbox_recomp2_executable::GuestAddress32;

/// A link from an instruction in this function to a known function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct FunctionReference {
    /// Address of the instruction that refers to the target.
    pub site: GuestAddress32,

    /// Referenced function's entry address.
    pub target: GuestAddress32,

    /// How the instruction refers to that function.
    pub kind: ReferenceKind,
}

/// A reached instruction whose possible destinations are not all known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct UnresolvedTransfer {
    /// Address of the instruction.
    pub site: GuestAddress32,

    /// What the instruction does with control flow.
    pub kind: TransferKind,
}

/// How an (walked) instruction refers to a known function entry.
///
/// [`DirectCall`]: Self::DirectCall
/// [`TailCall`]: Self::TailCall
/// [`PossiblePointer`]: Self::PossiblePointer
/// [`BruteForceKind`]: super::BruteForceKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReferenceKind {
    /// `call 0x2000` calls a directly named function.
    DirectCall,

    /// Enters another known function without adding a return address.
    ///
    /// - `jmp 0x2000`: a direct jump.
    /// - Taken `jnz 0x2000`: a conditional branch.
    /// - A switch case targeting `0x2000`: a resolved case.
    TailCall,

    /// A scanned instruction at `site` that may use the entry's address.
    ///
    /// e.g.
    ///
    /// ```asm
    /// push 0x10000 ; <- func address
    /// ```
    ///
    /// ```asm
    /// mov eax, 0x10000 ; <- func address
    /// ```
    PossiblePointer,
}

/// How control can leave an instruction when not every destination is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) enum TransferKind {
    /// `call eax` may return to the next instruction.
    Call,

    /// `jmp eax` does not continue at the next instruction.
    Jump,

    /// An unknown branch target still leaves a not-taken path.
    Conditional,

    /// The walker cannot follow flow such as `jmp far [eax]`.
    Unsupported,
}
