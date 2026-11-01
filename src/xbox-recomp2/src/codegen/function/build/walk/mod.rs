//! Walks over a function's instructions, one 'step' per instruction.
//!
//! # Usage
//!
//! Call [`WalkType::next_step`] until it returns [`None`].
//! Each call gives back one [`WalkStep`].
//!
//! # Choosing the instructions
//!
//! [`WalkType`] holds one of two walks:
//!
//! - [`InstructionWalk`] decodes every byte from `base` to `base + size`.
//! - [`SelectedWalk`] visits only the instructions listed in a [`FunctionBody`].
//!
//! The former is used for functions manually defined by user.
//! The latter is used by automatic discovery.
//!
//! # What a step contains
//!
//! Here's what the walks return:
//!
//! - [`WalkStep`] is one instruction.
//! - [`BlockExit`] says what to write after a step.
//! - [`StepFailure`] says why a step has no decoded instruction.
//!
//! [`FunctionBody`]: crate::analysis::FunctionBody

use crate::decode::{Insn, Status};
pub(super) use linear::InstructionWalk;
pub(super) use selected::SelectedWalk;
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

mod linear;
mod selected;

// ── Error Messages ───────────────────────────────────────────────────────────

/// The instruction's bytes failed to decode.
const ERROR_DECODE_BARRIER: &str = "Selected instruction failed to decode";
/// The bytes decode to a different length than the body stored.
const ERROR_LENGTH_MISMATCH_BARRIER: &str =
    "Selected instruction length disagrees with its decoded bytes";
/// The instruction's stored length goes past the end of its section or
/// of the file.
const ERROR_TRUNCATED_BARRIER: &str = "Selected instruction runs past its section or file end";
/// No section contains the instruction's address, or the file has no
/// bytes there.
const ERROR_UNMAPPED_BARRIER: &str = "Selected instruction is not mapped in the image";

/// One instruction visited by a walk.
pub(super) struct WalkStep<'a> {
    /// Address of the instruction.
    pub(super) addr: GuestAddress32,

    /// Whether this instruction ends a block.
    pub(super) is_last_instruction: bool,

    /// Whether this is the last step of the whole function walk.
    pub(super) is_final_step: bool,

    /// What to write after this instruction for flow control.
    /// i.e. Continue, goto, etc.
    pub(super) exit: BlockExit,

    /// The decoded instruction, or why it couldn't be decoded.
    pub(super) decoded: Result<&'a Insn, StepFailure>,
}

/// Which of the two walks is used for this function.
pub(super) enum WalkType<'a> {
    /// Decode every byte from `base` to `base + size`.
    Linear(InstructionWalk<'a>),

    /// Visit only the instructions in a [`FunctionBody`].
    ///
    /// [`FunctionBody`]: crate::analysis::FunctionBody
    Selected(SelectedWalk<'a>),
}

/// What to write after an instruction so C++ doesn't progress to the wrong
/// next block/instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlockExit {
    /// Nothing.
    Continue,

    /// `goto` this address. The next instruction to run isn't the next
    /// one.
    Goto(GuestAddress32),

    /// The block ends here, but not with a `jmp` or `ret`.
    /// Nothing should run after it, so we abort in case we do; because
    /// that kinda indicates we f*cked up.
    NoFallthrough,

    /// The instruction's bytes are unusable. Print this message and abort.
    Abort(&'static str),
}

/// Why a step has no decoded instruction.
///
/// [`FunctionBody`]: crate::analysis::FunctionBody
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepFailure {
    /// The bytes aren't a valid instruction.
    Decode(Status),

    /// No section contains the address, or the file has no bytes there.
    Unmapped,

    /// The instruction's `ins_length` bytes go past the end of the section or file.
    Truncated { ins_length: u32 },

    /// Analysis expected `ins_length` bytes, but they decode to `decoded_length`.
    LengthMismatch {
        ins_length: u32,
        decoded_length: u32,
    },
}

impl WalkType<'_> {
    /// Returns the next step, or `None` when the walk is done.
    pub(super) fn next_step(&mut self) -> Option<WalkStep<'_>> {
        match self {
            WalkType::Linear(walk) => walk.next_step(),
            WalkType::Selected(walk) => walk.next_step(),
        }
    }
}

impl StepFailure {
    /// Returns the message the generated code prints before aborting.
    fn barrier(self) -> &'static str {
        match self {
            StepFailure::Decode(_) => ERROR_DECODE_BARRIER,
            StepFailure::Unmapped => ERROR_UNMAPPED_BARRIER,
            StepFailure::Truncated { .. } => ERROR_TRUNCATED_BARRIER,
            StepFailure::LengthMismatch { .. } => ERROR_LENGTH_MISMATCH_BARRIER,
        }
    }

    /// Writes a message about this failure at `addr` to `diag`.
    pub(in super::super) fn report(self, addr: GuestAddress32, diag: &mut dyn Write) {
        let _ = match self {
            StepFailure::Decode(_) => writeln!(diag, "Failed to decode instruction at 0x{addr:X}"),
            StepFailure::Unmapped => writeln!(
                diag,
                "ERROR: Selected instruction at 0x{addr:X} is not mapped in the image"
            ),
            StepFailure::Truncated { ins_length } => writeln!(
                diag,
                "ERROR: Selected instruction at 0x{addr:X} stores length {ins_length}, \
                 which runs past its section or file end"
            ),
            StepFailure::LengthMismatch {
                ins_length,
                decoded_length,
            } => writeln!(
                diag,
                "ERROR: Selected instruction at 0x{addr:X} stores length {ins_length} \
                 but its bytes decode to {decoded_length}"
            ),
        };
    }
}
