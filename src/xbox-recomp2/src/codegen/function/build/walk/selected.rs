//! Visits only the instructions listed in a function body.

use super::{BlockExit, StepFailure, WalkStep};
use crate::analysis::FunctionBody;
use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder, Mnemonic};
use crate::executable::Executable;
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// Visits the instructions that analysis found for a function, in address order.
pub(in super::super) struct SelectedWalk<'a> {
    decoder: &'a Decoder,
    buffer: &'a mut DecodeBuffer,
    executable: &'a Executable,
    body: &'a FunctionBody,
    fallthroughs: &'a BTreeMap<GuestAddress32, GuestAddress32>,
    index: usize,
}

impl<'a> SelectedWalk<'a> {
    /// Creates a walk over the instructions in `body`.
    ///
    /// # Arguments
    ///
    /// * `decoder` - decodes the instructions.
    /// * `buffer` - reusable decode scratch space.
    /// * `executable` - source of the instruction bytes.
    /// * `body` - the instructions and how they connect.
    /// * `fallthroughs` - next instruction after each one, from
    ///   [`collect_labels_from_body`].
    ///
    /// [`collect_labels_from_body`]: super::super::labels::collect_labels_from_body
    pub(in super::super) fn new(
        decoder: &'a Decoder,
        buffer: &'a mut DecodeBuffer,
        executable: &'a Executable,
        body: &'a FunctionBody,
        fallthroughs: &'a BTreeMap<GuestAddress32, GuestAddress32>,
    ) -> Self {
        Self {
            decoder,
            buffer,
            executable,
            body,
            fallthroughs,
            index: 0,
        }
    }

    /// Returns the next step, or [`None`] after the last instruction.
    pub(super) fn next_step(&mut self) -> Option<WalkStep<'_>> {
        // Get virtual addresses for each instruction.
        let instruction_start_addresses = self.body.starts();
        if self.index >= instruction_start_addresses.len() {
            return None;
        }

        let ins_addr = instruction_start_addresses[self.index];
        let ins_length = u32::from(self.body.lengths()[self.index]);
        self.index += 1;
        let next_ins_addr = instruction_start_addresses.get(self.index).copied();
        let is_final_step = self.index >= instruction_start_addresses.len();

        // Decode exactly `ins_length` bytes, and check the decoder used them all.
        let decoded = match instruction_bytes(self.executable, ins_addr, ins_length) {
            Err(failure) => Err(failure),
            Ok(bytes) => match self.buffer.decode_at(self.decoder, bytes, 0) {
                DecodeOutcome::Failed { status, .. } => Err(StepFailure::Decode(status)),
                DecodeOutcome::Ok(insn) => match insn.length() as u32 {
                    decoded_length if decoded_length == ins_length => Ok(insn),
                    decoded_length => Err(StepFailure::LengthMismatch {
                        ins_length,
                        decoded_length,
                    }),
                },
            },
        };

        match decoded {
            Err(failure) => Some(WalkStep {
                addr: ins_addr,
                is_last_instruction: false,
                is_final_step,
                exit: BlockExit::Abort(failure.barrier()),
                decoded: Err(failure),
            }),
            Ok(insn) => Some(WalkStep {
                addr: ins_addr,
                is_last_instruction: !self.fallthroughs.contains_key(&ins_addr),
                is_final_step,
                exit: block_exit(
                    insn.mnemonic(),
                    next_ins_addr,
                    self.fallthroughs.get(&ins_addr),
                ),
                decoded: Ok(insn),
            }),
        }
    }
}

/// Decides what to write after an instruction.
///
/// - Falls through to the next instruction: nothing.
/// - Falls through elsewhere: `goto`.
/// - No fallthrough, `jmp` or `ret`: nothing. It leaves on its own.
/// - No fallthrough, anything else: [`BlockExit::NoFallthrough`].
///
/// This applies to the last instruction too.
/// Otherwise the C++ function would silently return.
fn block_exit(
    mnemonic: Mnemonic,
    next_ins_addr: Option<GuestAddress32>,
    fallthrough: Option<&GuestAddress32>,
) -> BlockExit {
    match fallthrough {
        Some(&target) => match next_ins_addr {
            Some(next_ins_addr) if target == next_ins_addr => BlockExit::Continue,
            _ => BlockExit::Goto(target),
        },
        None => match mnemonic {
            Mnemonic::JMP | Mnemonic::RET => BlockExit::Continue,
            _ => BlockExit::NoFallthrough,
        },
    }
}

/// Returns the `ins_length` bytes at `ins_addr`.
///
/// # Errors
///
/// - [`StepFailure::Unmapped`]: no section or file bytes at `ins_addr`.
/// - [`StepFailure::Truncated`]: fewer than `ins_length` bytes available.
fn instruction_bytes(
    executable: &Executable,
    ins_addr: GuestAddress32,
    ins_length: u32,
) -> Result<&[u8], StepFailure> {
    let data = executable
        .find(u64::from(ins_addr))
        .ok_or(StepFailure::Unmapped)?;

    // Must be within a section that's mapped by the .xbe
    data.get(..ins_length as usize)
        .ok_or(StepFailure::Truncated { ins_length })
}
