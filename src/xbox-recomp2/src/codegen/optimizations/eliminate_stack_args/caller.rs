//! Rewrites calls so their arguments arrive in registers.
//!
//! See `eliminate-stack-args.md` for more info.
//!
//! For a called function that wants argument 0 in `edx`:
//!
//! ```asm
//! push eax            ; argument 0; nothing reads this stack copy
//! mov  ecx, edi
//! call f
//! ```
//!
//! becomes
//!
//! ```asm
//! mov  ecx, edi
//! mov  edx, eax       ; (rewritten)
//! call f              ; moves ESP by 8: the removed push, then the return address
//! ```

use super::called::Called;
use crate::codegen::function::model::{
    ALL_ARITHMETIC_FLAGS, DirectionFlag, FunctionModel, GuestOp, Op,
};
use crate::codegen::optimizations::helpers::call_setup::CallSetup;
use crate::codegen::optimizations::helpers::entry_loads::EntryLoad;
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::optimizations::helpers::known_callers::direct_call_target;
use crate::codegen::optimizations::helpers::stack_frame::STACK_WORD_BYTES;
use crate::config::FunctionAddress;
use crate::decode::{
    Insn, Mnemonic, Operand, Register, RegisterClass, encode, immediate, register,
};
use std::collections::BTreeMap;

/// Where a register gets its argument from, just before the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgumentSource {
    /// It's already there. (e.g. __fastcall from OG compiler)
    InPlace,

    /// Copied from this register
    Register(Register),

    /// Set it to this constant.
    Immediate(u32),

    /// Load it from the argument's stack slot.
    Stack,
}

/// How many calls we rewrote, and how many pushes that removed.
#[derive(Default)]
pub(super) struct Rewritten {
    pub calls: usize,
    pub pushes: usize,
}

/// Rewrites each call in `model` to a function in `called`.
pub(super) fn rewrite_calls(
    model: &mut FunctionModel,
    called: &BTreeMap<FunctionAddress, Called>,
    rewritten: &mut Rewritten,
) {
    for block in &mut model.blocks {
        let ops = &mut block.ops;

        // Last call first.
        let mut index = ops.len();
        while index > 0 {
            index -= 1;
            let Op::Call(call) = &ops[index] else {
                continue;
            };
            let target = direct_call_target(&call.guest);
            let Some(function) = target.and_then(|target| called.get(&target)) else {
                continue;
            };

            // Setup walks stop at calls.
            let setup = function
                .call_setup(&ops[..index])
                .expect("retain checked every call");
            rewritten.pushes += rewrite(ops, index, function, &setup);
            rewritten.calls += 1;

            // No calls sit between the setup and this call.
            index = setup.start.get();
        }
    }
}

/// Rewrites the call at `call`, so `function` gets its arguments in registers.
fn rewrite(ops: &mut Vec<Op>, call: usize, function: &Called, setup: &CallSetup) -> usize {
    // Pick the argument sources first, before overwriting any instructions
    let sources = pick_sources(ops, call, function, setup);

    // Remove the argument writes nobody needs.
    let len = ops.len();
    let pushes = remove_writes(ops, call, function, setup, &sources);
    let call = call - (len - ops.len());

    // Have the call move ESP for the removed pushes too.
    let Op::Call(call_op) = &mut ops[call] else {
        unreachable!("removing writes keeps the call")
    };
    let moved = pushes as u32 * STACK_WORD_BYTES;
    call_op.stack_bytes += moved;
    call_op.calls_fastcall = true;
    let address = call_op.guest.address;

    // Register copies go first, so a later move can't overwrite a copy's source.
    let mut insns = Vec::with_capacity(function.loads.len() * 2);
    for copies in [true, false] {
        for (load, &source) in function.loads.iter().zip(&sources) {
            if matches!(source, ArgumentSource::Register(_)) == copies {
                argument_moves(load, source, moved, &mut insns);
            }
        }
    }

    // Put the argument moves right before the call.
    let moves = insns.into_iter().map(|insn| {
        Op::Guest(GuestOp {
            address,
            insn,
            switch_table: None,
            last_in_function: false,
            rewritten: true,
            needed_flags: ALL_ARITHMETIC_FLAGS,
            direction: DirectionFlag::Unknown,
        })
    });
    ops.splice(call..call, moves);
    pushes
}

/// Appends the argument moves that put `load`'s argument in its register.
fn argument_moves(load: &EntryLoad, source: ArgumentSource, moved: u32, insns: &mut Vec<Insn>) {
    let destination = Operand::Reg(load.register);
    let xmm = load.register.class() == RegisterClass::XMM;
    match source {
        ArgumentSource::InPlace => {}
        ArgumentSource::Register(from) if xmm => {
            insns.push(encode(Mnemonic::MOVD, &[destination, Operand::Reg(from)]));
        }
        ArgumentSource::Register(from) => {
            insns.push(encode(Mnemonic::MOV, &[destination, Operand::Reg(from)]));
        }
        ArgumentSource::Immediate(value) => {
            insns.push(encode(Mnemonic::MOV, &[destination, Operand::Imm(value)]));
        }
        ArgumentSource::Stack => {
            let disp = i32::from(load.argument) * STACK_WORD_BYTES as i32 - moved as i32;
            let slot = Operand::Mem {
                base: Register::ESP,
                disp,
            };
            if xmm {
                // Note: XMM registers take the value in their low 32 bits and
                // clear the rest, e.g. `movss xmm, [esp+N]`.
                insns.push(encode(Mnemonic::XORPS, &[destination; 2]));
                insns.push(encode(Mnemonic::MOVSS, &[destination, slot]));
            } else {
                insns.push(encode(Mnemonic::MOV, &[destination, slot]));
            }
        }
    }
}

/// Picks how each register gets its argument, in [`Called::loads`] order.
fn pick_sources(
    ops: &[Op],
    call: usize,
    function: &Called,
    setup: &CallSetup,
) -> Vec<ArgumentSource> {
    let mut sources = Vec::with_capacity(function.loads.len());
    let mut copied_into = Vec::with_capacity(function.loads.len());
    for load in &function.loads {
        let destination = load.register;
        let written = setup
            .captures
            .iter()
            .find(|capture| capture.argument == load.argument)
            .and_then(|capture| written_value(ops, capture.index.get(), call));

        let source = match written {
            Some(ArgumentSource::Register(from)) if from == destination => ArgumentSource::InPlace,

            // An earlier copy overwrote the source, like `mov eax, ecx` before `mov ecx, eax`.
            Some(ArgumentSource::Register(from)) if copied_into.contains(&from) => {
                ArgumentSource::Stack
            }
            Some(ArgumentSource::Immediate(_)) if destination.class() == RegisterClass::XMM => {
                ArgumentSource::Stack
            }
            Some(source) => source,
            None => ArgumentSource::Stack,
        };
        if matches!(source, ArgumentSource::Register(_)) {
            copied_into.push(destination);
        }
        sources.push(source);
    }
    sources
}

/// Removes the argument writes the called function wouldn't need
/// as a result of our rewrite of the call.
///
/// For example, if the called function wants argument 0 in eax:
///
/// | Before           | After          |
/// | ---------------- | -------------- |
/// | `push edx`       | `mov eax, edx` |
/// | `mov [esp], edx` | `mov eax, edx` |
fn remove_writes(
    ops: &mut Vec<Op>,
    call: usize,
    function: &Called,
    setup: &CallSetup,
    sources: &[ArgumentSource],
) -> usize {
    // Go from the back, so removing an op doesn't move the ones still to check.
    let mut pushes = 0;
    let mut esp_used = false;

    for index in (setup.start.get()..call).rev() {
        let insn = &guest(&ops[index]).insn;
        let push = is_push(insn);

        if is_removable(index, function, setup, sources) && !(push && esp_used) {
            ops.remove(index);
            pushes += usize::from(push);
        } else if RegisterAccess::of(insn).touches(Register::ESP) {
            esp_used = true;
        }
    }

    pushes
}

/// Whether the argument write at `index` is one the called function doesn't need.
fn is_removable(
    index: usize,
    function: &Called,
    setup: &CallSetup,
    sources: &[ArgumentSource],
) -> bool {
    let Some(capture) = setup
        .captures
        .iter()
        .find(|capture| capture.index.get() == index)
    else {
        return false;
    };

    let source = function
        .loads
        .iter()
        .position(|load| load.argument == capture.argument)
        .map(|load| sources[load]);

    capture.omit_write && source.is_some_and(|source| source != ArgumentSource::Stack)
}

/// Returns the constant or register `ops[write]` writes, if that register
/// still holds it at `call`.
fn written_value(ops: &[Op], write: usize, call: usize) -> Option<ArgumentSource> {
    // `push src` or `mov [esp+N], src`.
    let insn = &guest(&ops[write]).insn;
    let source = if is_push(insn) { 0 } else { 1 };
    let source = &insn.operands()[source];
    if let Some(value) = immediate(source) {
        return Some(ArgumentSource::Immediate(value.value as u32));
    }

    let source = register(source)
        .filter(|reg| reg.class() == RegisterClass::GPR32 && *reg != Register::ESP)?;
    let unchanged = ops[write + 1..call]
        .iter()
        .all(|op| !RegisterAccess::of(&guest(op).insn).writes(source));
    unchanged.then_some(ArgumentSource::Register(source))
}

/// The guest instruction in `op`. Setup only contains guest instructions.
fn guest(op: &Op) -> &GuestOp {
    let Op::Guest(guest) = op else {
        unreachable!("setup only contains guest instructions")
    };
    guest
}

/// Whether `insn` is a `push`.
fn is_push(insn: &Insn) -> bool {
    insn.mnemonic() == Mnemonic::PUSH
}

#[cfg(test)]
mod tests {
    use super::super::tests::listing;
    use super::*;
    use crate::codegen::optimizations::helpers::stack_arguments::{ArgumentSet, StackArguments};
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    /// A function with `ret N` that wants argument `n` in `registers[n]`.
    fn called(registers: &[Register]) -> Called {
        Called {
            arguments: StackArguments {
                count: registers.len() as u8,
                caller_cleanup: false,
            },
            loads: (0..)
                .zip(registers)
                .map(|(argument, &register)| EntryLoad {
                    argument,
                    register,
                    address: 0x1040.into(),
                })
                .collect(),
            kept: ArgumentSet::empty(),
        }
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    /// Each case is the setup before a `call`.
    /// `wanted` is the register for each argument.
    ///
    /// `call N` is how far the call moves ESP: 4 for the return address, plus
    /// 4 for each removed push.
    #[rstest]
    #[case::copy(
        &[
            0x50,                   // push eax             ; argument 0
            0x89, 0xF9,             // mov  ecx, edi
        ],
        &[Register::EDX],
        &["mov ecx, edi", "mov edx, eax", "call 8"],
    )]
    #[case::in_place(
        &[
            0x50,                   // push eax             ; already in eax
        ],
        &[Register::EAX],
        &["call 8"],
    )]
    #[case::constant(
        &[
            0x6A, 0,                // push 0
        ],
        &[Register::EAX],
        &["mov eax, 0x00", "call 8"],
    )]
    #[case::xmm(
        &[
            0x50,                   // push eax
        ],
        &[Register::XMM0],
        &["movd xmm0, eax", "call 8"],
    )]
    #[case::mov_write(
        &[
            0x83, 0xEC, 4,          // sub  esp, 4
            0x89, 0x04, 0x24,       // mov  [esp], eax      ; doesn't move ESP: goes
        ],
        &[Register::EDX],
        &["sub esp, 0x04", "mov edx, eax", "call 4"],
    )]
    #[case::copy_before_constant(
        &[
            0x50,                   // push eax             ; argument 1
            0x6A, 5,                // push 5               ; argument 0
        ],
        &[Register::EAX, Register::EDX],
        // `mov eax, 5` first would overwrite argument 1's source. <- copy goes first
        &["mov edx, eax", "mov eax, 0x05", "call 12"],
    )]
    fn rewrite_should_move_arguments_and_remove_writes(
        #[case] setup: &[u8],
        #[case] wanted: &[Register],
        #[case] expected: &[&str],
    ) {
        assert_eq!(rewrite_call(setup, wanted), expected);
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    /// Each case keeps an argument write, and may load it back from the stack.
    #[rstest]
    #[case::esp_used_after(
        &[
            0x50,                   // push eax
            0x8D, 0x0C, 0x24,       // lea  ecx, [esp]      ; <- uses ESP: push stays
        ],
        &[Register::EDX],
        &["push eax", "lea ecx, [esp]", "mov edx, eax", "call 4"],
    )]
    #[case::source_changed(
        &[
            0x50,                   // push eax
            0xB8, 1, 0, 0, 0,       // mov  eax, 1          ; <- eax no longer holds it
        ],
        &[Register::EDX],
        &["push eax", "mov eax, 0x01", "mov edx, [esp]", "call 4"],
    )]
    #[case::xmm_constant(
        &[
            0x6A, 0,                // push 0               ; <- no constant moves into xmm
        ],
        &[Register::XMM0],
        &["push 0x00", "xorps xmm0, xmm0", "movss xmm0, dword ptr [esp]", "call 4"],
    )]
    #[case::offset_after_removed_push(
        &[
            0x6A, 0,                // push 0               ; argument 1: stays
            0x50,                   // push eax             ; argument 0: goes
        ],
        &[Register::EDX, Register::XMM0],
        // With argument 0's push gone, argument 1 sits at [esp]. <- not [esp+4]
        &["push 0x00", "mov edx, eax", "xorps xmm0, xmm0", "movss xmm0, dword ptr [esp]", "call 8"],
    )]
    #[case::slot_above_esp(
        &[
            0x83, 0xEC, 8,          // sub  esp, 8
            0x89, 0x4C, 0x24, 4,    // mov  [esp+4], ecx    ; argument 1
            0xB9, 1, 0, 0, 0,       // mov  ecx, 1          ; <- ecx no longer holds it
            0x89, 0x14, 0x24,       // mov  [esp], edx      ; argument 0: goes
        ],
        &[Register::EAX, Register::EDX],
        // No push removed, so argument 1 is still at [esp+4]. <- [esp+0x04]
        &[
            "sub esp, 0x08", "mov [esp+0x04], ecx", "mov ecx, 0x01",
            "mov eax, edx", "mov edx, [esp+0x04]", "call 4",
        ],
    )]
    #[case::swapped(
        &[
            0x50,                   // push eax             ; argument 1
            0x51,                   // push ecx             ; argument 0
        ],
        &[Register::EAX, Register::ECX],
        // `mov eax, ecx` overwrites argument 1's source. <- load it instead
        &["push eax", "mov eax, ecx", "mov ecx, [esp]", "call 8"],
    )]
    fn rewrite_should_keep_writes_it_still_needs(
        #[case] setup: &[u8],
        #[case] wanted: &[Register],
        #[case] expected: &[&str],
    ) {
        assert_eq!(rewrite_call(setup, wanted), expected);
    }

    /// Rewrites `setup` followed by a `call`, for a function that wants `wanted`.
    ///
    /// Returns the rewritten listing.
    fn rewrite_call(setup: &[u8], wanted: &[Register]) -> Vec<String> {
        // Arrange
        let mut code = setup.to_vec();
        code.extend_from_slice(&[0xE8, 0, 0, 0, 0]); // call
        let mut ops = model_of(&code).blocks.swap_remove(0).ops;
        let call = ops.len() - 1;
        let function = called(wanted);
        let setup = function.call_setup(&ops[..call]).unwrap();

        // Act
        rewrite(&mut ops, call, &function, &setup);
        listing(&ops)
    }
}
