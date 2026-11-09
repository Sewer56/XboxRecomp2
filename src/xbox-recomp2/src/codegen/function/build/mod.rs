//! Builds the [`FunctionModel`] for one guest function.
//!
//! - [`build`] picks a walk, runs it and groups the steps into blocks.
//! - [`walk`] holds the two ways to pick the instructions:
//!   - [`InstructionWalk`] decodes every byte of the range.
//!   - [`SelectedWalk`] visits only the instructions in a [`FunctionBody`].
//! - [`labels`] finds the addresses that need labels, and the hooks.

use super::model::{
    ALL_ARITHMETIC_FLAGS, Block, Call, DirectionFlag, Entry, Exit, FunctionAbi, FunctionModel,
    GuestOp, Hook, Op, Return,
};
use crate::analysis::{Function, FunctionBody};
use crate::codegen::optimizations::helpers::stack_frame::STACK_WORD_BYTES;
use crate::config::{FunctionAddress, RecompilerConfig};
use crate::decode::{DecodeBuffer, Decoder, Mnemonic, immediate};
use crate::executable::Executable;
use labels::{LabelPass, collect_labels, collect_labels_from_body};
use std::collections::BTreeSet;
use std::io::Write;
pub(crate) use walk::StepFailure;
use walk::{BlockExit, InstructionWalk, SelectedWalk, WalkStep, WalkType};
use xbox_recomp2_executable::GuestAddress32;

mod labels;
mod walk;

/// Builds the model for `function`.
///
/// # Arguments
///
/// * `executable` - where the instruction bytes come from.
/// * `config` - banned and invalid ranges, switch tables and hooks.
/// * `decoder` - decodes the instructions.
/// * `buffer` - a reusable decode buffer.
/// * `function` - the function's address and size.
/// * `body` - the instructions to use, or `None` for the whole range.
/// * `diag` - receives the hook warnings.
///
/// # Returns
///
/// The function's blocks, hooks, scratch variables and entry.
///
/// # Remarks
///
/// See `recompiling-functions.md` for more detail, but the most important
/// parts for you to know are:
///
/// - We only reuse instructions from `body` (mid func data is skipped)
/// - Config can overwrite func lengths.
/// - `diag` reports anything that goes bad.
pub(super) fn build(
    executable: &Executable,
    config: &RecompilerConfig,
    decoder: &Decoder,
    buffer: &mut DecodeBuffer,
    function: &Function,
    body: Option<&FunctionBody>,
    diag: &mut dyn Write,
) -> FunctionModel {
    // Read the raw data from the .xbe
    let base = GuestAddress32::from(function.base.get() as u32);
    let size = function.size as u32;
    let data = executable.find(u64::from(base.get())).unwrap_or(&[]);

    // Find jump targets that need labels, and any hooks.
    let pass: LabelPass = match body {
        Some(body) => collect_labels_from_body(config, body, diag),
        None => collect_labels(config, decoder, buffer, data, base, size, diag),
    };

    // Now produce the model. Thanks.
    let mut model = FunctionModel {
        address: FunctionAddress(base.get()),
        entry: body.map_or(Entry::First, entry_of),
        mask_branch_targets: body.is_some(),
        // We need to disable some optimisations for replaced functions.
        // Namely, things that affect ABI, since hooks expect original
        // ABI to stay.
        abi: if config.function_hooks.contains(&FunctionAddress(base.get())) {
            FunctionAbi::Replaced
        } else {
            FunctionAbi::Original
        },
        ..FunctionModel::default()
    };

    let mut walk = match body {
        // Declared in `fixes.functions`: no instruction list, so decode
        // every byte in the range.
        None => WalkType::Linear(InstructionWalk::new(
            config, decoder, buffer, data, base, size,
        )),
        // Found by analysis: visit only the instructions it found.
        Some(body) => WalkType::Selected(SelectedWalk::new(
            decoder,
            buffer,
            executable,
            body,
            &pass.fallthroughs,
        )),
    };
    while let Some(step) = walk.next_step() {
        add_step(&mut model.blocks, config, &pass.labels, step);
    }

    // Every function stays in memory until the end, so give back spare room.
    for block in &mut model.blocks {
        block.ops.shrink_to_fit();
    }
    model.blocks.shrink_to_fit();

    // Finally get all the hooks and their info.
    model.hooks = pass
        .hook_addresses
        .into_iter()
        .filter(|&address| {
            config
                .midasm_hooks
                .get(FunctionAddress(address.get()))
                .is_some()
        })
        .map(|address| Hook { address })
        .collect();
    model.hooks.shrink_to_fit();
    model
}

/// Adds one walk step to the blocks.
///
/// A step starts a new block when it has a label, or when the previous
/// block already ends with a `goto` or an abort.
///
/// # Arguments
///
/// * `blocks` - the blocks built so far.
/// * `config` - switch tables, looked up at the step's address.
/// * `labels` - addresses needing a label.
/// * `step` - the step to add.
fn add_step(
    blocks: &mut Vec<Block>,
    config: &RecompilerConfig,
    labels: &BTreeSet<GuestAddress32>,
    step: WalkStep<'_>,
) {
    // Start a new block at a label...
    // or after a block that already ended.
    let at_label = labels.contains(&step.addr);
    let needs_block = at_label || blocks.last().is_none_or(|block| block.exit != Exit::Next);
    if needs_block {
        blocks.push(Block {
            label: at_label.then_some(step.addr),
            ops: Vec::new(),
            exit: Exit::Next,
        });
    }

    // Add the instruction, or the failure, to the current block.
    let block = blocks.last_mut().expect("block was just ensured");
    block.ops.push(match step.decoded {
        Ok(insn) => {
            let guest = GuestOp {
                address: step.addr,
                insn: insn.clone(),
                switch_table: config
                    .switch_tables
                    .get(FunctionAddress(step.addr.get()))
                    .cloned(),
                last_in_function: step.is_last_instruction && step.is_final_step,
                rewritten: false,
                needed_flags: ALL_ARITHMETIC_FLAGS,
                direction: DirectionFlag::Unknown,
            };
            if insn.mnemonic() == Mnemonic::CALL {
                // We can do some optimisations around CALLs
                // like not adjusting stack pointer.
                // So we wrap in own intrinsic here.
                Op::Call(Call {
                    guest,
                    stack_bytes: STACK_WORD_BYTES,
                    calls_fastcall: false,
                })
            } else if insn.mnemonic() == Mnemonic::RET {
                // Same with RET as with CALL
                Op::Return(Return {
                    argument_bytes: immediate(&insn.operands()[0])
                        .map_or(0, |value| value.value as u16),
                    guest,
                    omit_return_slot: false,
                })
            } else {
                Op::Guest(guest)
            }
        }
        Err(failure) => Op::Failed {
            address: step.addr,
            failure,
        },
    });

    // Set how the block ends, so control can't fall into the wrong block.
    block.exit = match step.exit {
        BlockExit::Continue => Exit::Next,
        BlockExit::Goto(target) => Exit::Goto(target),
        BlockExit::NoFallthrough => Exit::NoFallthrough { address: step.addr },
        BlockExit::Abort(message) => Exit::Abort {
            message,
            address: step.addr,
        },
    };
}

/// Decides how a function generated from `body` starts.
fn entry_of(body: &FunctionBody) -> Entry {
    let entry = body.entry();
    if body.first_address() == Some(entry) {
        Entry::First
    } else if body.contains(entry) {
        Entry::Goto(entry)
    } else {
        Entry::Missing(entry)
    }
}
