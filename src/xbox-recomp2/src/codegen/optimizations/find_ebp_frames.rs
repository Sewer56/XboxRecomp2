//! Finds functions that use EBP as the frame pointer.
//!
//! See `find-ebp-frames.md` for more info.

use super::helpers::instruction::control_transfer::leaves_block;
use super::helpers::instruction::register_access::RegisterAccess;
use super::helpers::known_callers::direct_call_target;
use crate::codegen::function::model::{Call, Entry, FunctionAbi, FunctionModel, GuestOp, Op};
use crate::config::FunctionAddress;
use crate::decode::{Mnemonic, Register, immediate, memory, register};
use ahash::HashSet;
use core::iter;

pub(super) fn run(models: &mut [FunctionModel]) {
    // Find the helpers that set up EBP for their caller first.
    let helpers: HashSet<FunctionAddress> = models
        .iter()
        .filter(|model| is_frame_helper(model))
        .map(|model| model.address)
        .collect();

    // And set the `ebp_frame` flag on each function.
    for model in models.iter_mut() {
        model.ebp_frame = uses_ebp_frame(model, &helpers);
    }
}

/// Whether `model` is an SEH (structured exception handling) helper,
/// like MSVC's `__SEH_prolog`.
///
/// These helpers set up EBP as the frame pointer when called.
/// Then the caller inherits that `EBP`.
///
/// Example in the markdown doc.
///
/// We spot one with following attributes:
///
/// - one block/run of instructions, ending in `ret`;
/// - its only EBP write is `mov ebp, esp` or `lea ebp, [esp+N]`.
///
/// This model is 'simplified', but for the functions we're aiming to pick up
/// from MSVC, it works.
fn is_frame_helper(model: &FunctionModel) -> bool {
    // Ignore host replacements.
    if model.abi == FunctionAbi::Replaced {
        return false;
    }

    // One block, entered on first instruction.
    let block = match model.blocks.as_slice() {
        [block] if model.entry == Entry::First => block,
        _ => return false,
    };

    // It must end in `ret`.
    let body = match block.ops.as_slice() {
        [body @ .., Op::Return(_)] => body,
        _ => return false,
    };

    // Exactly one EBP write, which points it at the stack.
    let mut sets_ebp = false;
    for op in body {
        let guest = match op {
            Op::Guest(guest) if !leaves_block(&guest.insn) => guest,
            _ => return false,
        };

        if !writes_ebp(guest) {
            continue;
        }

        if sets_ebp || !sets_ebp_from_esp(guest) {
            return false;
        }

        sets_ebp = true;
    }

    sets_ebp
}

/// Whether EBP points at `model`'s stack frame.
///
/// # Arguments
///
/// - `model`: the function to check.
/// - `helpers`: functions that set EBP for their caller. See [`is_frame_helper`].
///
/// # Returns
///
/// `true` if the function:
///
/// - Starts by setting up the frame, see [`frame_setup_len`].
/// - Writes EBP nowhere else, except to clean up right before a `ret`.
///
/// # Remarks
///
/// We assume `cdecl`, `stdcall` or variation thereof is in play, i.e.
/// `EBP` is preserved across function calls.
fn uses_ebp_frame(model: &FunctionModel, helpers: &HashSet<FunctionAddress>) -> bool {
    if model.entry != Entry::First {
        return false;
    }

    // The first block runs first, so it must set up the frame.
    let first = match model.blocks.first() {
        Some(block) => block.ops.as_slice(),
        None => return false,
    };
    let setup_len = match frame_setup_len(first, helpers) {
        Some(len) => len,
        None => return false,
    };

    // After that, EBP may only change right before a `ret`.
    for (block_index, block) in model.blocks.iter().enumerate() {
        let block_op_start_idx = if block_index == 0 { setup_len } else { 0 };

        for (index, op) in block.ops.iter().enumerate().skip(block_op_start_idx) {
            if !keeps_frame(op, &block.ops[index + 1..], helpers) {
                return false;
            }
        }
    }
    true
}

/// How many ops at the start of `first` set up the frame.
///
/// # Returns
///
/// The op count for either of these, or [`None`]:
///
/// - `push ebp`, then `mov ebp, esp` or `lea ebp, [esp+N]`.
/// - A call to one of `helpers`, with no jump or EBP access before it.
fn frame_setup_len(first: &[Op], helpers: &HashSet<FunctionAddress>) -> Option<usize> {
    // Inline prologue.
    if let [Op::Guest(push), Op::Guest(set), ..] = first
        && is_push_ebp(push)
        && sets_ebp_from_esp(set)
    {
        return Some(2);
    }

    // SEH prologue, e.g. `mov eax, handler; call helper`.
    for (index, op) in first.iter().enumerate() {
        match op {
            Op::Call(call) if calls_helper(call, helpers) => return Some(index + 1),
            Op::Guest(guest)
                if !leaves_block(&guest.insn)
                    && !RegisterAccess::of(&guest.insn).touches(Register::EBP) => {}
            _ => return None,
        }
    }
    None
}

/// Whether EBP still points at the frame for any code that runs after `op`.
///
/// `after` is the rest of `op`'s block.
fn keeps_frame(op: &Op, after: &[Op], helpers: &HashSet<FunctionAddress>) -> bool {
    match op {
        Op::Guest(guest) => !writes_ebp(guest) || restores_before_ret(guest, after),
        Op::Call(call) => !calls_helper(call, helpers),
        Op::Return(_) => true,
        Op::CdqIdiv(idiv) => !writes_ebp(&idiv.guest),
        Op::MergedCmpJmp(merged) => {
            let merged_ops = merged.instructions.iter().map(|merged| &merged.guest);
            let mut guests = iter::once(&merged.compare)
                .chain(&merged.x87_pop)
                .chain(merged_ops);
            !guests.any(writes_ebp)
        }
        Op::Failed { .. } => false,
    }
}

/// Whether `guest` is part of an epilogue that ends in the `ret` in `after`.
///
/// Accepts:
///
/// - `pop ebp` or `leave`, right before `ret`.
/// - `add ebp, N` or `sub ebp, N`, then one of those.
fn restores_before_ret(guest: &GuestOp, after: &[Op]) -> bool {
    match after {
        [Op::Return(_), ..] => restores_ebp(guest),
        [Op::Guest(restore), Op::Return(_), ..] => adjusts_ebp(guest) && restores_ebp(restore),
        _ => false,
    }
}

/// Whether `call` goes straight to one of `helpers`.
fn calls_helper(call: &Call, helpers: &HashSet<FunctionAddress>) -> bool {
    direct_call_target(&call.guest).is_some_and(|target| helpers.contains(&target))
}

/// Whether `guest` is `push ebp`.
fn is_push_ebp(guest: &GuestOp) -> bool {
    let insn = &guest.insn;
    insn.mnemonic() == Mnemonic::PUSH && register(&insn.operands()[0]) == Some(Register::EBP)
}

/// Whether `guest` is `mov ebp, esp` or `lea ebp, [esp+N]`.
fn sets_ebp_from_esp(guest: &GuestOp) -> bool {
    let insn = &guest.insn;
    let operands = insn.operands();
    let source = match insn.mnemonic() {
        Mnemonic::MOV => register(&operands[1]),
        Mnemonic::LEA => memory(&operands[1])
            .filter(|mem| mem.index == Register::NONE)
            .map(|mem| mem.base),
        _ => return false,
    };

    register(&operands[0]) == Some(Register::EBP) && source == Some(Register::ESP)
}

/// Whether `guest` is `add ebp, N` or `sub ebp, N`.
fn adjusts_ebp(guest: &GuestOp) -> bool {
    let insn = &guest.insn;
    matches!(insn.mnemonic(), Mnemonic::ADD | Mnemonic::SUB)
        && register(&insn.operands()[0]) == Some(Register::EBP)
        && immediate(&insn.operands()[1]).is_some()
}

/// Whether `guest` is `pop ebp` or a 32-bit `leave`.
fn restores_ebp(guest: &GuestOp) -> bool {
    let insn = &guest.insn;
    match insn.mnemonic() {
        Mnemonic::POP => register(&insn.operands()[0]) == Some(Register::EBP),
        Mnemonic::LEAVE => insn.instruction().operand_width == 32,
        _ => false,
    }
}

/// Whether `guest` writes any part of EBP.
fn writes_ebp(guest: &GuestOp) -> bool {
    RegisterAccess::of(&guest.insn).writes(Register::EBP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    #[rstest]
    #[case::pop_before_ret(
        &[
            0x55,                   //     push ebp
            0x89, 0xE5,             //     mov  ebp, esp
            0xE8, 0, 0, 0, 0,       //     call .next      ; calls keep EBP
                                    // .next:
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0x5D,                   //     pop  ebp
            0xC3,                   //     ret
        ],
        true,
    )]
    #[case::leave_before_ret(
        &[
            0x55,                   //     push ebp
            0x89, 0xE5,             //     mov  ebp, esp
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0xC9,                   //     leave
            0xC3,                   //     ret
        ],
        true,
    )]
    #[case::lea_frame(
        &[
            0x55,                   //     push ebp
            0x8D, 0x6C, 0x24, 0x90, //     lea  ebp, [esp-0x70]
            0x8B, 0x45, 0x78,       //     mov  eax, [ebp+0x78]
            0x83, 0xC5, 0x70,       //     add  ebp, 0x70  ; undo the offset
            0xC9,                   //     leave           ; 'leave' is right after, so dajiobu
            0xC3,                   //     ret
        ],
        true,
    )]
    #[case::adjusted_too_early(
        &[
            0x55,                   //     push ebp
            0x8D, 0x6C, 0x24, 0x90, //     lea  ebp, [esp-0x70]
            0x83, 0xC5, 0x70,       //     add  ebp, 0x70  ; <- not followed by leave, hence ebp invalid nyow~
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0xC9,                   //     leave
            0xC3,                   //     ret
        ],
        false,
    )]
    #[case::no_frame(
        &[
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0xC3,                   //     ret
        ],
        false,
    )]
    #[case::overwritten(
        &[
            0x55,                   //     push ebp
            0x89, 0xE5,             //     mov  ebp, esp
            0x89, 0xC5,             //     mov  ebp, eax   ; <- EBP overwritten, nyo good
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0x5D,                   //     pop  ebp
            0xC3,                   //     ret
        ],
        false,
    )]
    #[case::pop_before_use(
        &[
            0x55,                   //     push ebp
            0x89, 0xE5,             //     mov  ebp, esp
            0x5D,                   //     pop  ebp        ; <- nyot followed by ret
            0x8B, 0x45, 0x08,       //     mov  eax, [ebp+8]
            0xC3,                   //     ret
        ],
        false,
    )]
    fn uses_ebp_frame_should_need_frame_until_ret(#[case] code: &[u8], #[case] expected: bool) {
        // Arrange
        let model = model_of(code);

        // Act
        let actual = uses_ebp_frame(&model, &HashSet::default());

        // Assert
        assert_eq!(actual, expected);
    }

    // ── SEH prologue helpers ─────────────────────────────────────────────────

    #[rstest]
    #[case::sets_caller_frame(
        &[
            0x89, 0x6C, 0x24, 0x04, //     mov  [esp+4], ebp 
            0x8D, 0x6C, 0x24, 0x04, //     lea  ebp, [esp+4]
            0xC3,                   //     ret
        ],
        true,
    )]
    #[case::own_frame(
        &[
            0x55,                   //     push ebp
            0x89, 0xE5,             //     mov  ebp, esp
            0x5D,                   //     pop  ebp
            0xC3,                   //     ret
        ],
        false,
    )]
    #[case::early_exit(
        &[
            0xE3, 0x10,             //     jecxz elsewhere ; <- skips setting EBP
            0x8D, 0x6C, 0x24, 0x04, //     lea  ebp, [esp+4]
            0xC3,                   //     ret
        ],
        false,
    )]
    fn is_frame_helper_should_need_single_ebp_write(#[case] code: &[u8], #[case] expected: bool) {
        // Arrange
        let model = model_of(code);

        // Act
        let actual = is_frame_helper(&model);

        // Assert
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::helper_sets_frame(
        &[
            0xB8, 0x78, 0x56, 0x34, 0x12, //     mov  eax, 0x12345678 ; handler table
            0xE8, 0, 0, 0, 0,             //     call helper
            0x8B, 0x45, 0x08,             //     mov  eax, [ebp+8]
            0xC9,                         //     leave
            0xC3,                         //     ret
        ],
        true,
    )]
    #[case::ebp_used_before_helper(
        &[
            0x8B, 0x45, 0x08,             //     mov  eax, [ebp+8] ; <- EBP isn't set yet
            0xE8, 0, 0, 0, 0,             //     call helper
            0x8B, 0x45, 0x08,             //     mov  eax, [ebp+8]
            0xC9,                         //     leave
            0xC3,                         //     ret
        ],
        false,
    )]
    #[case::helper_skipped(
        &[
            0xE3, 0x05,                   //     jecxz .after      ; <- skips the helper
            0xE8, 0, 0, 0, 0,             //     call helper
                                          // .after:
            0x8B, 0x45, 0x08,             //     mov  eax, [ebp+8]
            0xC9,                         //     leave
            0xC3,                         //     ret
        ],
        false,
    )]
    fn uses_ebp_frame_should_accept_helper_prologue(#[case] code: &[u8], #[case] expected: bool) {
        // Arrange
        let model = model_of(code);
        let helpers = model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .filter_map(|op| match op {
                Op::Call(call) => direct_call_target(&call.guest),
                _ => None,
            })
            .collect();

        // Act
        let actual = uses_ebp_frame(&model, &helpers);

        // Assert
        assert_eq!(actual, expected);
    }
}
