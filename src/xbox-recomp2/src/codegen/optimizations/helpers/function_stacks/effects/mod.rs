//! Works out what calling a function does to ESP and EBP.
//!
//! - [`walked`]: for a function we could walk.
//! - [`trusted`]: for one we couldn't, by trusting its `ret N`.
//!
//! This is harder to follow than needs to be, and a bit ugly :/
//! I'll need to clean this up, but alas, time constraints. - Sewer

use super::super::stack_frame::STACK_WORD_BYTES;
use crate::codegen::function::model::Return;
use crate::decode::{BranchType, Mnemonic};
pub(super) use trusted::trusting_ret_n;
pub(super) use walked::from_walk;

mod trusted;
mod walked;

/// Returns how many bytes one `ret` pops.
/// Includes the return address, if there is one.
pub(super) fn popped_by(ret: &Return) -> Option<u32> {
    // Only regular 32-bit `ret`s.
    let insn: &zydis::ffi::DecodedInstruction = ret.guest.insn.instruction();
    if ret.guest.insn.mnemonic() != Mnemonic::RET
        || insn.operand_width != 32
        || insn.meta.branch_type != BranchType::NEAR
    {
        return None;
    }

    // N argument bytes + return address
    let return_address = if ret.omit_return_slot {
        0
    } else {
        STACK_WORD_BYTES
    };
    Some(u32::from(ret.argument_bytes) + return_address)
}
