//! Questions about a single instruction, used by the other helpers.
//!
//! - [`control_transfer`]: jumps, calls, returns and other ways out of a block.
//! - [`effects`]: flags written/read by instructions. (eflags, mmx, x87 etc.)
//! - [`fpu`]: whether an instruction is x87 or MMX code.
//! - [`general_registers`]: a set of 32-bit general-purpose registers.
//! - [`register_access`]: which registers an instruction reads or writes.
//! - [`stack_slot`]: whether a memory operand points into the stack.
//! - [`plain_work`]: simple instructions that leave ESP alone, so walks can
//!   step past them.

pub(in crate::codegen) mod control_transfer;
pub(in crate::codegen) mod effects;
pub(in crate::codegen) mod fpu;
pub(in crate::codegen) mod general_registers;
pub(in crate::codegen) mod plain_work;
pub(in crate::codegen) mod register_access;
pub(in crate::codegen) mod stack_slot;
