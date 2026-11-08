//! Questions about a single instruction, used by the other helpers.
//!
//! - [`control_transfer`]: jumps, calls, returns and other ways out of a block.
//! - [`flag_effects`]: arithmetic flag reads and definite (always) writes.
//! - [`register_access`]: which registers an instruction reads or writes.
//! - [`plain_work`]: simple instructions that leave ESP alone, so walks can
//!   step past them.

pub(in crate::codegen) mod control_transfer;
pub(in crate::codegen) mod flag_effects;
pub(in crate::codegen) mod plain_work;
pub(in crate::codegen) mod register_access;
