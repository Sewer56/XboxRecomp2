//! Guest instructions that need special handling for optimisations.
//!
//! - [`Call`]: a `call` instruction.
//! - [`Return`]: a `ret` instruction.
//! - [`MergedCmpJmp`]: a compare and the conditional jump that reads it.

pub(crate) use call::Call;
pub(crate) use merged_cmp_jmp::{
    Condition, MergedCmpJmp, MergedInstruction, MergedInstructionKind,
};
pub(crate) use return_op::Return;

mod call;
mod merged_cmp_jmp;
mod return_op;
