//! Decode guest x86 instructions.
//!
//! - [`DecodeBuffer`] decodes bytes into an [`Insn`] using a [`Decoder`].
//! - `RangeDecoder` decodes every instruction in a `fixes.functions` range.
//! - [`DisasmFormatter`] turns an [`Insn`] into Intel-style text.
//! - `encode` builds a new [`Insn`] for optimisation passes that rewrite code.

pub use decoder::{DecodeBuffer, DecodeOutcome, Decoder};
pub(crate) use encode::{Operand, encode};
pub use format::{DISASM_BUFFER_SIZE, DisasmFormatter};
pub use insn::{Insn, immediate, immediate_signed, memory, register};
pub(crate) use range::RangeDecoder;
pub use zydis::ffi::{
    DecodedInstruction, DecodedOperand, DecodedOperandKind, DisplacementInfo, ImmediateInfo,
    MemoryInfo, PointerInfo,
};
pub(crate) use zydis::{BranchType, CpuFlag, InstructionCategory, MachineMode};
pub use zydis::{
    ElementType, InstructionAttributes, Mnemonic, OperandAction, OperandAttributes,
    OperandVisibility, Register, RegisterClass, Status,
};

mod decoder;
mod encode;
mod format;
mod insn;
mod range;
