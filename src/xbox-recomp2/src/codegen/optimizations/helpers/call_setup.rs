//! Finds arguments written to the stack before a call.
//!
//! See `call-setup.md` for more info.
//!
//! ```asm
//! sub  esp, 8             ; stack adjustment: keep going
//! mov  [esp], eax         ; writes argument 0
//! mov  [esp+4], edx       ; writes argument 1
//! call f
//! ```
//!
//! - [`CallSetup::find`]: returns the argument writes before a call instruction.

use super::instruction::plain_work::leaves_esp_alone;
use super::stack_arguments::{ArgumentSet, StackArguments};
use super::stack_frame::{STACK_WORD_BYTES, StackFrame};
use crate::codegen::function::model::{BlockOpIndex, Op};
use crate::decode::{
    Insn, MemoryInfo, Mnemonic, Register, RegisterClass, immediate, memory, register,
};
use derive_more::Constructor;
use optional::Optioned;

/// The instructions that write one call's arguments.
pub(in crate::codegen) struct CallSetup {
    /// Earliest write.
    pub start: BlockOpIndex,

    /// One per argument found, closest to the call first.
    pub captures: Vec<Capture>,
}

/// A write to the stack.
#[derive(Clone, Copy, Constructor)]
struct StackWrite {
    /// Offset of the write, relative to ESP at the call.
    offset: i32,

    /// Width of the write in bits.
    bits: u16,
}

/// The write closest to the call that fills one whole argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) struct Capture {
    /// The write.
    pub index: BlockOpIndex,

    /// Argument number. 0 is `[esp]` at the call.
    pub argument: u8,

    /// Nothing reads the write, so a pass may remove it.
    pub omit_write: bool,
}

impl CallSetup {
    /// Finds the write closest to the call for each wanted argument.
    ///
    /// # Arguments
    ///
    /// - `ops`: the block of ops containing the call, up to the call.
    /// - `stack`: the stack arguments of the callee (called function).
    /// - `wanted`: the arguments to find writes for.
    /// - `kept`: arguments whose writes stay even if nothing in `ops` reads
    ///   them, because the callee or caller reads the stack slot later.
    ///
    /// # Returns
    ///
    /// The setup, or [`None`] if no wanted argument has a capturable write.
    ///
    /// # Remarks
    ///
    /// We leave an argument out if its closest write is partial,
    /// like `mov byte [esp], 1`.
    pub(in crate::codegen) fn find(
        ops: &[Op],
        stack: StackArguments,
        mut wanted: ArgumentSet,
        mut kept: ArgumentSet,
    ) -> Option<Self> {
        // ESP just after the current instruction, relative to ESP at the call.
        // So argument 0 is at offset 0.

        // Note: We don't use function_stack here, because it can't analyse
        //       every single call. Instead; we analyse just a little slice
        //       before the 'call', allowing us to have better reliability.

        let mut esp_offset = 0i32;
        let mut captures = Vec::new();

        // Iterate back from the 'call' instruction.
        for (index, op) in ops.iter().enumerate().rev() {
            // If unsupported instruction, bail out.
            let Op::Guest(guest) = op else {
                break;
            };

            // Skip 16-bit instructions
            let insn = &guest.insn;
            if insn.instruction().address_width != 32 {
                break;
            }

            // Is this a push of a 32-bit value?
            let push = insn.mnemonic() == Mnemonic::PUSH && insn.instruction().operand_width == 32;
            let write = stack_write(insn, push, esp_offset); // a write to stacc? OwO

            // Capture whole-argument writes of:
            // - A 32-bit register, like `mov [esp], eax`.
            // - A constant, like `push 1`.
            // - An XMM register's low 32 bits, like `movss [esp], xmm0`.
            // - 32-bit memory, like `push [ecx]`.
            //
            // Other writes leave their arguments on the stack.
            if let Some(StackWrite { offset, bits }) = write {
                let source = if push { 0 } else { 1 };

                // Get argument index for this write.
                let argument =
                    whole_argument(stack, offset, bits).filter(|_| capturable_source(insn, source));

                match argument {
                    Some(argument) => {
                        let flag = ArgumentSet::argument(argument);
                        if wanted.contains(flag) {
                            // Record a capture if this argument is one we want.
                            captures.push(Capture {
                                index: BlockOpIndex::new(index),
                                argument,
                                omit_write: !kept.contains(flag),
                            });
                            wanted.remove(flag);
                        }
                    }
                    None => wanted.remove(stack.overlap(offset, bits)),
                }
            }

            // If this was a stack push, we need to offset esp for
            // previous instructions.
            let read_esp = if push {
                esp_offset.checked_add(STACK_WORD_BYTES as i32)?
            } else {
                esp_offset
            };

            // Note which args was read here.
            // So an earlier write to it can stay.
            //
            // i.e. edit `kept` so we don't touch this reg in previous args.
            let mov_write = !push && write.is_some();
            kept |= read_arguments(insn, stack, read_esp, mov_write);

            if wanted.is_empty() {
                break;
            }

            // Step back past this instruction, if we understand it.
            if push && write.is_some() {
                esp_offset = read_esp;
            } else if let Some(delta) = StackFrame::adjustment(insn) {
                esp_offset = esp_offset.checked_sub(delta)?;
            } else if !leaves_esp_alone(insn, write.is_some()) {
                break;
            }
        }

        // `captures` is closest to the call first (left to right order, in C)
        // so we want the first item as it's at first address.
        let start = captures.last()?.index;
        Some(Self { start, captures })
    }
}

/// Finds the stack arguments that `insn` reads.
///
/// # Arguments
///
/// - `stack`: the callee's stack arguments.
/// - `read_esp`: ESP when `insn` reads, relative to ESP at the call.
///   For `push`, that's ESP before the push.
/// - `mov_write`: whether `insn` is `mov [esp+N], src`, so operand 0 is a write.
///
/// # Returns
///
/// The arguments read.
/// [`ArgumentSet::all`] if an address is unknown, like `[edx]`.
fn read_arguments(
    insn: &Insn,
    stack: StackArguments,
    read_esp: i32,
    mov_write: bool,
) -> ArgumentSet {
    // `lea` only computes an address. It doesn't read it.
    if insn.mnemonic() == Mnemonic::LEA {
        return ArgumentSet::empty();
    }

    // Skip hidden operands, like the slot `push` writes.
    let visible = usize::from(insn.instruction().operand_count_visible);
    let operands = insn.operands().iter().take(visible);

    // Skip the write's destination.
    let mut read = ArgumentSet::empty();
    for operand in operands.skip(usize::from(mov_write)) {
        let Some(mem) = memory(operand) else {
            continue;
        };
        read |= match stack_offset(mem, read_esp) {
            Some(offset) => stack.overlap(offset, operand.size),
            None => ArgumentSet::all(), // Unknown address.
        };
    }
    read
}

/// Finds where `insn` writes the stack.
///
/// Only these count:
/// - `push src`, if we can capture `src`.
/// - `mov [esp+N], src` or `movss [esp+N], src`.
///
/// # Arguments
///
/// - `push`: whether `insn` is a 32-bit `push`.
/// - `esp_offset`: ESP just after `insn`, relative to ESP at the call.
///
/// # Returns
///
/// The write, or [`None`] if `insn` isn't one of the above.
fn stack_write(insn: &Insn, push: bool, esp_offset: i32) -> Option<StackWrite> {
    // A push writes 32 bits at the new ESP.
    if push {
        // Index 0 for immediate, e.g. `push 0x123456`
        return capturable_source(insn, 0).then_some(StackWrite::new(esp_offset, 32));
    }

    // Like `mov [esp+4], eax`.
    if !matches!(insn.mnemonic(), Mnemonic::MOV | Mnemonic::MOVSS) {
        return None;
    }
    let destination = &insn.operands()[0];
    let offset = stack_offset(memory(destination)?, esp_offset)?;
    Some(StackWrite::new(offset, destination.size))
}

/// Returns the argument that a write at `offset` fills exactly.
///
/// [`None`] if the write:
/// - Isn't 32 bits.
/// - Doesn't start on an argument, like `[esp+2]`.
/// - Is outside the arguments.
fn whole_argument(stack: StackArguments, offset: i32, bits: u16) -> Option<u8> {
    let word = STACK_WORD_BYTES as i32;
    if bits != 32 || offset < 0 || offset % word != 0 {
        return None;
    }

    let argument = offset / word;
    (argument < i32::from(stack.count)).then_some(argument as u8)
}

/// Whether operand `index` is a whole 32-bit value we can capture.
///
/// That's one of:
/// - An immediate or a 32-bit register.
/// - For `movss`: the low 32 bits of an XMM register.
/// - For `push`: a 32-bit read from DS or SS memory.
fn capturable_source(insn: &Insn, index: usize) -> bool {
    let operand = &insn.operands()[index];
    let mnemonic = insn.mnemonic();

    // Like `push 1`.
    if immediate(operand).is_some() {
        return true;
    }

    // Like `eax`, or `xmm0` in `movss`.
    if let Some(reg) = register(operand) {
        return match reg.class() {
            RegisterClass::GPR32 => operand.size == 32,
            RegisterClass::XMM => mnemonic == Mnemonic::MOVSS,
            _ => false,
        };
    }

    // Like `push [ecx]`.
    memory(operand).is_some_and(|mem| {
        mnemonic == Mnemonic::PUSH
            && operand.size == 32
            && matches!(mem.segment, Register::DS | Register::SS)
    })
}

/// Turns `[esp+N]` into an offset from ESP at the call.
///
/// `esp_offset` is ESP at this instruction, relative to ESP at the call.
/// Returns [`None`] for any other address, like `[edx]` or `[esp+eax]`.
fn stack_offset(mem: &MemoryInfo, esp_offset: i32) -> Option<i32> {
    StackFrame {
        esp: esp_offset,
        ebp: Optioned::none(),
    }
    .address(mem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    /// A capture whose write nothing reads.
    fn skippable(index: usize, argument: u8) -> Capture {
        Capture {
            index: BlockOpIndex::new(index),
            argument,
            omit_write: true,
        }
    }

    /// A capture whose write something reads.
    fn kept(index: usize, argument: u8) -> Capture {
        Capture {
            index: BlockOpIndex::new(index),
            argument,
            omit_write: false,
        }
    }

    /// The callee takes two arguments.
    ///
    /// Each case lists the captures in the order `find` returns them, closest
    /// to the call first.
    #[rstest]
    #[case::pushes(
        &[
            0x50,                       // push eax             ; argument 1
            0x52,                       // push edx             ; argument 0
        ],
        &[skippable(1, 0), skippable(0, 1)],
    )]
    #[case::allocated(
        &[
            0x83, 0xEC, 8,              // sub  esp, 8
            0x89, 0x04, 0x24,           // mov  [esp], eax      ; argument 0
            0x89, 0x54, 0x24, 4,        // mov  [esp+4], edx    ; argument 1
        ],
        &[skippable(2, 1), skippable(1, 0)],
    )]
    #[case::overwritten(
        &[
            0x83, 0xEC, 8,              // sub  esp, 8
            0x89, 0x04, 0x24,           // mov  [esp], eax      ; overwritten below
            0x89, 0x14, 0x24,           // mov  [esp], edx      ; argument 0
        ],
        &[skippable(2, 0)],
    )]
    #[case::read_back(
        &[
            0x50,                       // push eax             ; argument 0
            0x8B, 0x0C, 0x24,           // mov  ecx, [esp]      ; reads it back
        ],
        &[kept(0, 0)],
    )]
    #[case::unknown_read(
        &[
            0x50,                       // push eax             ; argument 0
            0x8B, 0x0A,                 // mov  ecx, [edx]      ; could read it
        ],
        &[kept(0, 0)],
    )]
    #[case::partial_write(
        &[
            0x50,                       // push eax
            0xC6, 0x04, 0x24, 1,        // mov  byte [esp], 1   ; partial: leave on stack
        ],
        &[],
    )]
    #[case::unknown_write(
        &[
            0x50,                       // push eax
            0x89, 0x02,                 // mov  [edx], eax      ; stop: unknown write
        ],
        &[],
    )]
    #[case::short_stack_pointer(
        &[
            0x50,                       // push eax
            0x66, 0x89, 0xC4,           // mov  sp, ax          ; stop: writes ESP
        ],
        &[],
    )]
    #[case::stack_pointer_value(
        &[
            0x54,                       // push esp             ; argument 0
        ],
        &[skippable(0, 0)],
    )]
    #[case::memory_after_push(
        &[
            0x50,                       // push eax             ; argument 1
            0xFF, 0x32,                 // push [edx]           ; argument 0, could read argument 1
        ],
        &[skippable(1, 0), kept(0, 1)],
    )]
    #[case::math(
        &[
            0x50,                       // push eax             ; argument 0
            0x83, 0xC1, 1,              // add  ecx, 1          ; register work
        ],
        &[skippable(0, 0)],
    )]
    #[case::compare_memory(
        &[
            0x50,                       // push eax             ; argument 0
            0x39, 0x0C, 0x24,           // cmp  [esp], ecx      ; reads it back
        ],
        &[kept(0, 0)],
    )]
    #[case::math_store(
        &[
            0x50,                       // push eax
            0x01, 0x0A,                 // add  [edx], ecx      ; stop: writes memory
        ],
        &[],
    )]
    fn find_should_return_last_argument_writes(#[case] code: &[u8], #[case] expected: &[Capture]) {
        // Arrange.
        let model = model_of(code);
        let stack = StackArguments {
            count: 2,
            caller_cleanup: false,
        };
        let wanted = ArgumentSet::ARG0 | ArgumentSet::ARG1;

        // Act.
        let setup = CallSetup::find(&model.blocks[0].ops, stack, wanted, ArgumentSet::empty());

        // Assert.
        let actual = setup.map(|setup| setup.captures).unwrap_or_default();
        assert_eq!(actual, expected);
    }
}
