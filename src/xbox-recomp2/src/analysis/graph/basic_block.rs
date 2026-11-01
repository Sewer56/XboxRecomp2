//! Instructions in each block, and the paths to other blocks.
//!
//! A block are all instructions until a branch or divergence is hit:
//!
//! ```asm
//! 0x1000: mov eax, [ebx]  ; ┐
//! 0x1002: test eax, eax   ; │ block 0x1000
//! 0x1004: jnz 0x1009      ; ┘
//! 0x1006: xor eax, eax    ; ┐
//! 0x1008: ret             ; ┘ block 0x1006
//! 0x1009: inc eax         ; ┐
//! 0x100A: ret             ; ┘ block 0x1009
//! ```
//!
//! The first block stops at `jnz`, because execution can go to either
//! `0x1006` (not taken) or `0x1009` (taken).
//!
//! Each destination starts its a new block. Ending @ `ret` or branch.

use xbox_recomp2_executable::GuestAddress32;

/// A straight-line run of instructions in one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct BasicBlock {
    /// Address of the first instruction.
    pub start: GuestAddress32,

    /// Instruction addresses in execution order.
    pub instructions: Vec<GuestAddress32>,

    /// Edges leaving this block's last instruction.
    ///
    /// For the `jnz` above, they lead to `0x1006` and `0x1009`.
    /// A `ret` block has none.
    ///
    /// Edges are sorted by target address, then kind.
    pub successors: Vec<LocalEdge>,
}

/// A path between instructions in the same function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) struct LocalEdge {
    /// Instruction taking this path.
    pub source: GuestAddress32,

    /// First instruction at the destination.
    pub target: GuestAddress32,

    /// How execution reaches the destination.
    pub kind: EdgeKind,
}

/// How execution gets from one instruction to another in the same function.
///
/// This branch gives `0x1007` two paths:
///
/// ```asm
/// 0x1007: jnz 0x1010  ; BranchTaken -> 0x1010
///                     ; BranchNotTaken -> 0x1009
/// 0x1009: ret
/// 0x1010: ret
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::analysis) enum EdgeKind {
    /// Continue at the next instruction, after ordinary code or returning
    /// from a `call`.
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

    /// Follow a direct `jmp 0x1010` inside this function.
    Jump,

    /// Follow a case address read from a table by `jmp [eax*4+0x4000]`.
    SwitchCase,
}

impl EdgeKind {
    /// Whether this path can become a tail call
    /// (when it hits a known function entry).
    pub fn can_tail_call(self) -> bool {
        matches!(self, Self::BranchTaken | Self::Jump | Self::SwitchCase)
    }
}
