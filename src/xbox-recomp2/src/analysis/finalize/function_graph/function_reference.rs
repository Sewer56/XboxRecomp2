//! Describe how one instruction refers to another function.

use crate::analysis::graph::ReferenceKind as DiscoveryReferenceKind;
use xbox_recomp2_executable::GuestAddress32;

/// A link from an instruction in this function to a known function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionReference {
    /// Entry address of the function containing the reference.
    pub source: GuestAddress32,

    /// Address of the instruction that refers to the target.
    pub site: GuestAddress32,

    /// Referenced function's entry address.
    pub target: GuestAddress32,

    /// How the instruction refers to that function.
    pub kind: ReferenceKind,
}

/// How an instruction refers to a known function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReferenceKind {
    /// `call 0x2000` calls a directly named function.
    DirectCall,

    /// Enters another known function without adding a return address.
    ///
    /// - `jmp 0x2000`: a direct jump.
    /// - Taken `jnz 0x2000`: a conditional branch.
    /// - A switch case targeting `0x2000`: a resolved case.
    ///
    /// This describes guest control flow; it does not guarantee host tail-call
    /// optimisation or a constant host stack depth.
    TailCall,

    /// A possible pointer to a function.
    ///
    /// For example:
    ///
    /// ```asm
    /// push 0x20000 ; <- points to code
    /// ```
    ///
    /// Or a reference by pointer in a table.
    ///
    /// This is a guess.
    PossiblePointer,
}

impl ReferenceKind {
    /// Whether this reference is a guess rather than something we know 100%.
    ///
    /// # Returns
    ///
    /// `true` for [`PossiblePointer`], which comes from the pointer scan.
    /// `false` for [`DirectCall`] and [`TailCall`], which the walk followed.
    ///
    /// # Remarks
    ///
    /// Required for optimisations which change calling convention/ABI.
    /// We can only safely change callconv when all callers to a mathod
    /// are known to be statically determined calls.
    ///
    /// [`PossiblePointer`]: Self::PossiblePointer
    /// [`DirectCall`]: Self::DirectCall
    /// [`TailCall`]: Self::TailCall
    pub fn is_guess(self) -> bool {
        self == Self::PossiblePointer
    }
}

impl From<DiscoveryReferenceKind> for ReferenceKind {
    fn from(kind: DiscoveryReferenceKind) -> Self {
        match kind {
            DiscoveryReferenceKind::DirectCall => Self::DirectCall,
            DiscoveryReferenceKind::TailCall => Self::TailCall,
            DiscoveryReferenceKind::PossiblePointer => Self::PossiblePointer,
        }
    }
}
