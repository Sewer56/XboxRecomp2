//! Spots memory operands that point into the current thread's stack.

use crate::decode::{MemoryInfo, Register};

/// Whether `mem` is a guest stack access.
///
/// That's `[esp+N]`, or `[ebp+N]` with `ebp_frame`.
///
/// # Arguments
///
/// - `mem`: the memory operand, like `[esp+4]`.
/// - `ebp_frame`: whether EBP is the function's frame pointer.
pub(in crate::codegen) fn is_stack_slot(mem: &MemoryInfo, ebp_frame: bool) -> bool {
    let ebp_slot = ebp_frame && mem.base == Register::EBP && mem.index == Register::NONE;
    let stack_base = mem.base == Register::ESP || ebp_slot;
    stack_base && mem.segment == Register::SS
}
