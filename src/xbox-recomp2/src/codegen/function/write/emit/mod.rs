//! Turns decoded instructions into C++.
//!
//! - [`lookup`] finds the emitter for a mnemonic.
//! - [`Emitter`] holds the instruction and the [`CodeWriter`] it writes to.
//!
//! Platform-owned memory goes through [`crate::platform::GuestPlatform`].
//! Helpers shared across families are in `operands`.
//!
//! Fixed text around functions lives in [`crate::templates`].

pub(super) use context::{Emitter, FunctionState};
pub(in crate::codegen) use instructions::supports_addressing;
pub(super) use instructions::{emit_call, emit_return, lookup, merged_cmp_jmp};
use operands::{array_subscript, constant};
use operands::{
    decode_memory_operand_no_parentheses, ea, get_x86_reg_float, is_mm0_to_7, is_xmm0_to_7,
    mmx_index, operand_reg_value, operand_type_name, segment_equals, st_index,
};
pub(in crate::codegen::function::write) use operands::{get_x86_reg, xmm_index};
pub(in crate::codegen::function) use writer::CodeWriter;
use writer::cpp;

mod context;
mod instructions;
mod operands;
mod writer;
