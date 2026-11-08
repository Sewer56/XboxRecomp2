//! Guest instructions that need special handling for optimisations.
//!
//! - [`Call`]: a `call` instruction.
//! - [`Return`]: a `ret` instruction.
//! - [`MergedCmpJmp`]: a compare and the conditional jump that reads it.
//! - [`CdqIdiv`]: an `idiv r/m32` right after `cdq`.

pub(crate) use call::Call;
pub(crate) use cdq_idiv::CdqIdiv;
pub(crate) use merged_cmp_jmp::{
    Condition, MergedCmpJmp, MergedInstruction, MergedInstructionKind,
};
pub(crate) use return_op::Return;

mod call;
mod cdq_idiv;
mod merged_cmp_jmp;
mod return_op;
