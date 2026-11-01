//! Tracks where ESP and EBP point as a function runs.
//!
//! Positions are byte offsets from ESP at one chosen instruction.
//!
//! That's the function's entry when walking a whole function, or a `call`
//! when looking at its arguments.
//!
//! Assuming you start `ESP +0` in both cases, after `push esi`, `ESP -4`.
//!
//! - [`StackFrame`] holds the positions and steps through instructions.
//! - [`FrameStep`] says what kind of stack instruction a step was.

use super::instruction::register_access::RegisterAccess;
use crate::decode::{
    Insn, MemoryInfo, Mnemonic, Register, RegisterClass, immediate, immediate_signed, memory,
    register,
};
use optional::Optioned;

/// Size of one stack slot, in bytes.
pub(in crate::codegen) const STACK_WORD_BYTES: u32 = 4;

/// What a stack instruction did, as reported by [`StackFrame::step`].
///
/// - `Unchanged`: ESP didn't move, though EBP may have become unknown.
/// - `Push`/`Pop`: pushed or popped a 32-bit register.
/// - `Adjust`: `add esp, N`, `sub esp, N`, `push imm` or `push [mem]`.
/// - `SetBase`: `mov ebp, esp`.
/// - `Restore`: `mov esp, ebp` or `leave`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) enum FrameStep {
    Unchanged,
    Push(Register),
    Pop(Register),
    Adjust,
    SetBase,
    Restore,
}

/// Where ESP and EBP point, as offsets from the starting ESP.
///
/// `ebp` is none when we don't know where EBP points.
/// [`Optioned`] stores none as [`i32::MIN`], so [`StackFrame::step`] gives up
/// rather than track EBP at that offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::codegen) struct StackFrame {
    pub esp: i32,
    pub ebp: Optioned<i32>,
}

impl StackFrame {
    /// Turns `[esp+N]` or `[ebp+N]` into an offset from the starting ESP.
    ///
    /// Returns [`None`] if the address:
    ///
    /// - uses another base, an index register or an unknown EBP;
    /// - uses a segment other than DS or SS.
    pub(in crate::codegen) fn address(self, mem: &MemoryInfo) -> Option<i32> {
        if mem.index != Register::NONE || !matches!(mem.segment, Register::DS | Register::SS) {
            return None;
        }
        let base = match mem.base {
            Register::ESP => self.esp,
            Register::EBP => self.ebp.into_option()?,
            _ => return None,
        };

        // Wraps like the CPU's 32-bit address maths.
        Some(base.wrapping_add(mem.disp.displacement as i32))
    }

    /// Updates ESP and EBP for one instruction, and says what it did.
    ///
    /// Other writes to EBP, like `mov ebp, eax`, make EBP unknown.
    ///
    /// Returns [`None`] if `insn`:
    ///
    /// - changes ESP in a way we can't follow;
    /// - puts a stack address in EBP;
    /// - uses 16-bit addressing;
    /// - moves an offset outside the `i32` range.
    ///
    /// `call` and `ret` aren't handled here. The caller must deal with them.
    pub(in crate::codegen) fn step(&mut self, insn: &Insn) -> Option<FrameStep> {
        if insn.instruction().address_width != 32 {
            return None;
        }

        // Work on a copy, so bailing out leaves `self` untouched.
        let mut next = *self;
        let ops = insn.operands();
        let dst = register(&ops[0]);
        let src = register(&ops[1]);
        let word = STACK_WORD_BYTES as i32;
        let is_32bit = insn.instruction().operand_width == 32;
        let gpr32 = dst.is_some_and(|reg| reg.class() == RegisterClass::GPR32);
        let step = match insn.mnemonic() {
            Mnemonic::PUSH if is_32bit && gpr32 => {
                next.esp = next.esp.checked_sub(word)?;
                FrameStep::Push(dst?)
            }
            Mnemonic::PUSH
                if is_32bit && (immediate(&ops[0]).is_some() || memory(&ops[0]).is_some()) =>
            {
                next.esp = next.esp.checked_sub(word)?;
                FrameStep::Adjust
            }
            Mnemonic::POP if is_32bit && gpr32 && dst != Some(Register::ESP) => {
                next.esp = next.esp.checked_add(word)?;
                if dst == Some(Register::EBP) {
                    next.ebp = Optioned::none();
                }
                FrameStep::Pop(dst?)
            }
            Mnemonic::MOV if dst == Some(Register::EBP) && src == Some(Register::ESP) => {
                // `i32::MIN` would read back as unknown.
                if next.esp == i32::MIN {
                    return None;
                }
                next.ebp = Optioned::some(next.esp);
                FrameStep::SetBase
            }
            Mnemonic::MOV if dst == Some(Register::ESP) && src == Some(Register::EBP) => {
                next.esp = next.ebp.into_option()?;
                FrameStep::Restore
            }
            Mnemonic::LEAVE if is_32bit => {
                next.esp = next.ebp.into_option()?.checked_add(word)?;
                next.ebp = Optioned::none();
                FrameStep::Restore
            }
            _ => match Self::adjustment(insn) {
                Some(delta) => {
                    next.esp = next.esp.checked_add(delta)?;
                    FrameStep::Adjust
                }
                None => {
                    next.other_write(insn)?;
                    FrameStep::Unchanged
                }
            },
        };

        *self = next;
        Some(step)
    }

    /// Handles instructions that [`Self::step`] has no specific case for.
    ///
    /// If `insn` writes EBP, EBP becomes unknown.
    ///
    /// Returns [`None`] if `insn`:
    ///
    /// - writes ESP;
    /// - puts a stack address in EBP, like `lea ebp, [esp+4]`;
    /// - computes EBP from its known value, like `add ebp, 4`.
    fn other_write(&mut self, insn: &Insn) -> Option<()> {
        // Any ESP write we don't recognise loses track of the stack.
        let access = RegisterAccess::of(insn);
        if access.writes(Register::ESP) {
            return None;
        }

        // Leaves EBP alone, so nothing changes.
        if !access.writes(Register::EBP) {
            return Some(());
        }

        // Does `insn` put a stack address in EBP that we can't follow?
        // If so, give up.
        //
        // Marking EBP unknown would hide that `[ebp+N]` still touches the stack.
        let ebp_tracked = self.ebp.is_some();

        // e.g. `add ebp, esp`, or `add ebp, 4` while EBP points into the stack.
        //       (tracked == true) -> points into stack.
        let esp_or_ebp_reads_stack =
            access.reads_value(Register::ESP) || (ebp_tracked && access.reads_value(Register::EBP));

        // e.g. `lea ebp, [esp+4]`, or `lea ebp, [ebp+4]` while EBP points into
        // the stack.
        let points_to_stack = |reg| reg == Register::ESP || (ebp_tracked && reg == Register::EBP);
        let lea_from_stack = insn.mnemonic() == Mnemonic::LEA
            && memory(&insn.operands()[1])
                .is_some_and(|mem| points_to_stack(mem.base) || points_to_stack(mem.index)); // base or reg point to stack

        if esp_or_ebp_reads_stack || lea_from_stack {
            return None;
        }

        // Otherwise EBP holds a plain value we don't track.
        self.ebp = Optioned::none();
        Some(())
    }

    /// Returns how much `add esp, N` or `sub esp, N` adds to ESP.
    ///
    /// For example, `sub esp, 8` gives -8.
    pub(in crate::codegen) fn adjustment(insn: &Insn) -> Option<i32> {
        // Must write all of ESP.
        if insn.instruction().operand_width != 32
            || register(&insn.operands()[0]) != Some(Register::ESP)
        {
            return None;
        }

        let value = i32::try_from(immediate_signed(&insn.operands()[1])?).ok()?;
        match insn.mnemonic() {
            Mnemonic::ADD => Some(value),
            Mnemonic::SUB => value.checked_neg(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::instruction;
    use rstest::rstest;

    /// Starts at ESP 0, EBP -8.
    ///
    /// `expected` is `Some((esp, ebp))` with the frame after the step.
    /// `ebp` is [`None`] when EBP becomes unknown.
    /// `expected` is [`None`] when `step` gives up, leaving the frame as is.
    #[rstest]
    #[case::push(&[0x56], Some((-4, Some(-8))))] // push esi
    #[case::push_immediate(&[0x6A, 1], Some((-4, Some(-8))))] // push 1
    #[case::allocate(&[0x83, 0xEC, 16], Some((-16, Some(-8))))] // sub esp, 16
    #[case::set_base(&[0x89, 0xE5], Some((0, Some(0))))] // mov ebp, esp
    #[case::restore(&[0x89, 0xEC], Some((-8, Some(-8))))] // mov esp, ebp
    #[case::leave(&[0xC9], Some((-4, None)))] // leave
    #[case::overwrite_base(&[0x89, 0xC5], Some((0, None)))] // mov ebp, eax
    #[case::partial_esp(&[0x66, 0x89, 0xC4], None)] // mov sp, ax
    #[case::dynamic(&[0x29, 0xC4], None)] // sub esp, eax
    #[case::pop_esp(&[0x5C], None)] // pop esp
    #[case::stack_address_from_esp(&[0x8D, 0x6C, 0x24, 4], None)] // lea ebp, [esp+4]
    #[case::stack_address_from_ebp(&[0x45], None)] // inc ebp
    fn step_should_track_esp_and_ebp(
        #[case] code: &[u8],
        #[case] expected: Option<(i32, Option<i32>)>,
    ) {
        // Arrange
        let start = StackFrame {
            esp: 0,
            ebp: Optioned::some(-8),
        };
        let mut frame = start;

        // Act
        let step = frame.step(&instruction(code));

        // Assert
        assert_eq!(step.is_some(), expected.is_some());
        let (esp, ebp) = expected.unwrap_or((start.esp, start.ebp.into_option()));
        assert_eq!(
            frame,
            StackFrame {
                esp,
                ebp: ebp.into()
            }
        );
    }
}
