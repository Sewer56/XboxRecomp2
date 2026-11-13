//! Finds a function's stack arguments and the instructions that load them.
//!
//! See `stack-arguments.md` for more info.
//!
//! - [`StackArguments::find`]: how many arguments, and who removes them?
//! - [`StackArguments::from_return`]: what does this `ret N` say about them?
//! - [`StackArguments::load`]: does this instruction load an argument?
//! - [`StackArguments::overlap`]: which arguments does this access touch?

bitflags! {
    /// A set of stack arguments, by number.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(in crate::codegen) struct ArgumentSet: u8 {
        const ARG0 = 1;
        const ARG1 = 1 << 1;
        const ARG2 = 1 << 2;
        const ARG3 = 1 << 3;
        const ARG4 = 1 << 4;
        const ARG5 = 1 << 5;
        const ARG6 = 1 << 6;
        const ARG7 = 1 << 7;
    }
}

use super::function_stack::FunctionStack;
use super::stack_frame::{STACK_WORD_BYTES, StackFrame};
use crate::decode::{
    BranchType, Insn, Mnemonic, Register, RegisterClass, immediate, memory, register,
};
use bitflags::bitflags;

/// The most arguments we handle. One per bit in [`ArgumentSet`].
pub(in crate::codegen) const MAX_STACK_ARGUMENTS: usize = u8::BITS as usize;

/// A function's stack arguments: how many, and who removes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::codegen) struct StackArguments {
    /// Number of 4-byte arguments, up to [`MAX_STACK_ARGUMENTS`].
    pub count: u8,

    /// Whether the caller removes the arguments after the call.
    ///
    /// `false` means the callee removes them, with `ret N`.
    ///
    /// Basically `true == cdecl`, `false == stdcall`
    pub caller_cleanup: bool,
}

impl StackArguments {
    /// Work out how many stack arguments a function takes, and who removes them.
    ///
    /// - `ret 8`: 2 arguments, removed by the callee.
    /// - Plain `ret`: removed by the caller.
    ///
    ///   We count up to the highest argument loaded.
    ///   For example, `mov eax, [esp+8]` at entry gives 2.
    ///
    /// Returns [`None`] if:
    ///
    /// - no `ret` is reachable;
    /// - a reachable `ret` is invalid (see [`StackArguments::from_return`]);
    /// - reachable `ret`s remove different numbers of bytes (sussy compiler stacc work)
    pub(in crate::codegen) fn find(stack: &FunctionStack<'_>) -> Option<Self> {
        match Self::return_bytes(stack)? {
            0 => Some(Self::from_loads(stack)), // caller cleanup (cdecl)
            bytes => Self::from_cleanup_bytes(bytes), // callee cleanup (stdcall)
        }
    }

    /// Calculates the number of bytes every reachable `ret` removes.
    ///
    /// Returns [`None`] if:
    /// - no `ret` is reachable
    /// - one is invalid
    /// - they disagree.
    fn return_bytes(stack: &FunctionStack<'_>) -> Option<u16> {
        let mut found = None;
        for item in &stack.instructions {
            // Skip if it's not a 'ret' or we don't have a frame.
            let (Some(ret), Some(_)) = (item.return_op, item.frame) else {
                continue;
            };

            // Make sure the `ret` is valid.
            Self::from_return(&ret.guest.insn)?;
            if found.is_some_and(|first| first != ret.argument_bytes) {
                return None; // disagrees with an existing 'ret', bail out.
            }

            // Update the last found item.
            found = Some(ret.argument_bytes);
        }
        found
    }

    /// Plain `ret`: counts up to the highest argument loaded.
    fn from_loads(stack: &FunctionStack<'_>) -> Self {
        // Accept any argument, so we see every load.
        let any_argument = Self {
            count: MAX_STACK_ARGUMENTS as u8,
            caller_cleanup: true,
        };

        let mut count = 0;
        for item in &stack.instructions {
            // Skip unreachable instructions.
            let Some(frame) = item.frame else {
                continue;
            };

            let insn = &item.instruction.insn;
            if let Some((argument, _)) = any_argument.load(insn, frame, stack.return_slot_bytes) {
                count = count.max(argument + 1);
            }
        }
        Self {
            count,
            caller_cleanup: true,
        }
    }

    /// Read stack arguments from a `ret`.
    ///
    /// - `ret 8`: 2 arguments, removed by the callee.
    /// - `ret`: removed by the caller, count unknown (0).
    ///
    /// Returns [`None`] if `insn`:
    ///
    /// - isn't a 32-bit near `ret`;
    /// - removes over 32 bytes, or not a multiple of 4.
    pub(in crate::codegen) fn from_return(insn: &Insn) -> Option<Self> {
        let is_32bit_near_ret = insn.mnemonic() == Mnemonic::RET
            && insn.instruction().operand_width == 32
            && insn.instruction().meta.branch_type == BranchType::NEAR;

        if !is_32bit_near_ret {
            return None;
        }

        // Plain `ret` is `ret 0`. `N` is 16-bit.
        let argument_bytes = immediate(&insn.operands()[0]).map_or(0, |imm| imm.value as u16);
        Self::from_cleanup_bytes(argument_bytes)
    }

    /// Turns the bytes a `ret` removes into arguments.
    ///
    /// Returns [`None`] if `argument_bytes` is over 32 or not divisible by 4.
    fn from_cleanup_bytes(argument_bytes: u16) -> Option<Self> {
        let bytes = u32::from(argument_bytes);
        let word = STACK_WORD_BYTES;
        if bytes > MAX_STACK_ARGUMENTS as u32 * word || !bytes.is_multiple_of(word) {
            return None;
        }
        Some(Self {
            count: (bytes / word) as u8,
            caller_cleanup: bytes == 0,
        })
    }

    /// Checks whether `insn` loads a whole argument into a register.
    ///
    /// Only `mov r32, [mem]` and `movss xmm, [mem]` count.
    /// For example, `mov eax, [esp+8]` at entry loads argument 1.
    ///
    /// # Arguments
    ///
    /// - `insn`: the instruction.
    /// - `frame`: ESP and EBP before `insn`.
    /// - `return_slot_bytes`: 4 with a return address, otherwise 0.
    ///
    /// # Returns
    ///
    /// The argument number and the register.
    ///
    /// [`None`] if `insn` isn't such a load, or reads an argument at or past
    /// `self.count`.
    pub(in crate::codegen) fn load(
        self,
        insn: &Insn,
        frame: StackFrame,
        return_slot_bytes: u32,
    ) -> Option<(u8, Register)> {
        // Check the instruction and its destination.
        let operands = insn.operands();
        let destination = register(&operands[0])?;
        let supported = match insn.mnemonic() {
            Mnemonic::MOV => destination.class() == RegisterClass::GPR32,
            Mnemonic::MOVSS => destination.class() == RegisterClass::XMM,
            _ => false,
        };
        if !supported || operands[1].size != 32 || insn.instruction().address_width != 32 {
            return None;
        }

        // Turn the address into an offset from argument 0 + argument number.
        let offset = frame
            .address(memory(&operands[1])?)?
            .checked_sub_unsigned(return_slot_bytes)?;
        let word = STACK_WORD_BYTES as i32;

        // Alignment check!
        if offset < 0 || offset % word != 0 {
            return None;
        }

        let argument = offset / word;
        (argument < i32::from(self.count)).then_some((argument as u8, destination))
    }

    /// Finds the arguments that an access touches.
    ///
    /// # Arguments
    ///
    /// - `offset`: bytes from the start of argument 0.
    /// - `bits`: access width, rounded up to whole bytes.
    ///
    /// # Example
    ///
    /// Each argument is 4 bytes.
    ///
    /// For example, a 16-bit access at offset 3 covers bytes 3 and 4,
    /// so it touches arguments 0 and 1:
    ///
    /// ```text
    /// byte:     | 0 1 2 3 | 4 5 6 7 |
    /// argument: |    0    |    1    |
    /// access:           ^^^^^
    /// ```
    ///
    pub(in crate::codegen) fn overlap(self, offset: i32, bits: u16) -> ArgumentSet {
        let word = STACK_WORD_BYTES as i32;
        let access_end = offset.saturating_add(i32::from(bits.div_ceil(8)));

        let mut touched = ArgumentSet::empty();
        for argument in 0..self.count {
            let start = i32::from(argument) * word;
            let end = start + word;

            // The access touches this argument if their byte ranges overlap.
            if offset < end && access_end > start {
                touched |= ArgumentSet::argument(argument);
            }
        }
        touched
    }
}

impl ArgumentSet {
    /// The set holding only argument number `argument` (0 to 7).
    pub(in crate::codegen) fn argument(argument: u8) -> Self {
        Self::from_bits_retain(1 << argument)
    }
}

#[cfg(test)]
mod tests {
    use super::super::function_stack::no_calls;
    use super::*;
    use crate::codegen::optimizations::helpers::tests::{analyzed_function, instruction};
    use rstest::rstest;

    /// `count` arguments, removed by the callee with `ret N`.
    fn callee(count: u8) -> Option<StackArguments> {
        Some(StackArguments {
            count,
            caller_cleanup: false,
        })
    }

    /// `count` arguments, removed by the caller after a plain `ret`.
    fn caller(count: u8) -> Option<StackArguments> {
        Some(StackArguments {
            count,
            caller_cleanup: true,
        })
    }

    /// ESP and EBP before an instruction, relative to ESP at entry.
    fn frame(esp: i32, ebp: Option<i32>) -> StackFrame {
        StackFrame {
            esp,
            ebp: ebp.into(),
        }
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    #[rstest]
    #[case::ret_8(
        &[
            0xC2, 8, 0,             // ret  8
        ],
        callee(2),
    )]
    #[case::ret(
        &[
            0xC3,                   // ret
        ],
        caller(0),
    )]
    #[case::ret_after_load(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0
            0xC3,                   // ret
        ],
        caller(1),
    )]
    #[case::ret_0_after_last_argument(
        &[
            0x8B, 0x44, 0x24, 32,   // mov  eax, [esp+32]   ; argument 7
            0xC2, 0, 0,             // ret  0
        ],
        caller(8),
    )]
    #[case::ret_after_too_many_arguments(
        &[
            0x8B, 0x44, 0x24, 36,   // mov  eax, [esp+36]   ; argument 8: too far
            0xC3,                   // ret
        ],
        caller(0),
    )]
    #[case::rets_disagree(
        &[
            0x85, 0xC9,             //     test ecx, ecx
            0x74, 3,                //     jz   .b
            0xC2, 4, 0,             //     ret  4
            0xC2, 8, 0,             // .b: ret  8
        ],
        None,
    )]
    fn find_should_read_ret_or_count_loads(
        #[case] code: &[u8],
        #[case] expected: Option<StackArguments>,
    ) {
        // Arrange.
        let model = analyzed_function(code);
        let stack = FunctionStack::new(&model, no_calls, false).unwrap();

        // Act.
        let arguments = StackArguments::find(&stack);

        // Assert.
        assert_eq!(arguments, expected);
    }

    /// Each case reads one argument, after a different opening.
    #[rstest]
    // (at entry)
    #[case::at_entry(
        &[0x8B, 0x44, 0x24, 4],     // mov  eax, [esp+4]
        frame(0, None),
        Some(0),
    )]
    // push esi
    #[case::after_push(
        &[0x8B, 0x44, 0x24, 8],     // mov  eax, [esp+8]
        frame(-4, None),
        Some(0),
    )]
    // push ebp
    // mov  ebp, esp
    // sub  esp, 12
    #[case::through_ebp(
        &[0x8B, 0x45, 12],          // mov  eax, [ebp+12]
        frame(-16, Some(-4)),
        Some(1),
    )]
    // (at entry)
    #[case::indexed(
        &[0x8B, 0x44, 0x8C, 4],     // mov  eax, [esp+ecx*4+4]  ; argument unknown
        frame(0, None),
        None,
    )]
    fn load_should_find_argument(
        #[case] code: &[u8],
        #[case] frame: StackFrame,
        #[case] expected: Option<u8>,
    ) {
        // Arrange.
        let arguments = StackArguments {
            count: 2,
            caller_cleanup: false,
        };

        // Act.
        let load = arguments.load(&instruction(code), frame, STACK_WORD_BYTES);

        // Assert.
        assert_eq!(load.map(|(argument, _)| argument), expected);
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    /// Two arguments, 4 bytes each:
    ///
    /// ```text
    /// byte:      -1 | 0 1 2 3 | 4 5 6 7 | 8
    /// argument:     |    0    |    1    |
    /// ```
    #[rstest]
    #[case::across_two(3, 16, ArgumentSet::ARG0 | ArgumentSet::ARG1)] // bytes 3-4
    #[case::starts_before(-1, 16, ArgumentSet::ARG0)] // bytes -1-0
    #[case::after_last(8, 32, ArgumentSet::empty())] // bytes 8-11
    fn overlap_should_find_touched_arguments(
        #[case] offset: i32,
        #[case] bits: u16,
        #[case] expected: ArgumentSet,
    ) {
        // Arrange.
        let arguments = StackArguments {
            count: 2,
            caller_cleanup: false,
        };

        // Act.
        let touched = arguments.overlap(offset, bits);

        // Assert.
        assert_eq!(touched, expected);
    }
}
