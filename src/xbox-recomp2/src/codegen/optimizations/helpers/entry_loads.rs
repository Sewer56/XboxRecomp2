//! Finds arguments read off the stack when a function starts.
//!
//! See `entry-loads.md` for more info.
//!
//! ```asm
//! push esi                ; stack setup: keep going
//! mov  eax, [esp+8]       ; entry load: argument 0 arrives in eax
//! test ecx, ecx           ; register work: keep going
//! mov  [ecx], eax         ; anything else: stop
//! ```
//!
//! - [`EntryLoad::find_all`]: walks forward from the entry and returns the loads.

use super::function_stack::{FunctionStack, is_branch};
use super::instruction::plain_work::leaves_esp_alone;
use super::instruction::register_access::RegisterAccess;
use super::stack_arguments::StackArguments;
use super::stack_frame::FrameStep;
use crate::decode::{Insn, Mnemonic, Register};
use xbox_recomp2_executable::GuestAddress32;

/// One argument the function loads before doing anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) struct EntryLoad {
    /// Which stack argument it reads.
    pub argument: u8,

    /// The register it's loaded into.
    pub register: Register,

    /// Guest address of the load instruction.
    pub address: GuestAddress32,
}

impl EntryLoad {
    /// Walks forward from the entry and collects the whole-argument loads.
    ///
    /// # Arguments
    ///
    /// - `stack`: the function's stack positions, from [`FunctionStack::new`].
    /// - `arguments`: the function's arguments, from [`StackArguments::find`].
    ///
    /// # Returns
    ///
    /// The loads, in the order they run. Empty if there are none.
    ///
    /// # Remarks
    ///
    /// Each load runs exactly once, and its register still holds the
    /// caller's value when the load runs.
    pub(in crate::codegen) fn find_all(
        stack: &FunctionStack<'_>,
        arguments: StackArguments,
    ) -> Vec<Self> {
        let merges = find_insns_with_more_than_one_path(stack);
        let mut loads = Vec::<Self>::new();
        let mut visited_instructions = Vec::<&Insn>::new();

        let mut index = stack.entry;
        loop {
            let item = &stack.instructions[index];
            let insn = &item.instruction.insn;
            let mnemonic = insn.mnemonic();

            // Stop at loops, joins, branches, calls and returns.
            if merges[index]
                || is_branch(mnemonic)
                || matches!(mnemonic, Mnemonic::RET | Mnemonic::CALL)
            {
                break;
            }
            let Some(mut frame) = item.frame else {
                break;
            };

            // Check if this instruction loads into register
            if let Some((argument, register)) = arguments.load(insn, frame, stack.return_slot_bytes)
            {
                // The register must still hold the caller's value.
                // i.e. Was not overwritten.
                let used_earlier = visited_instructions
                    .iter()
                    .any(|earlier| RegisterAccess::of(earlier).touches(register));

                let loaded_before = loads.iter().any(|load| load.argument == argument);
                if matches!(register, Register::ESP | Register::EBP)
                    || used_earlier
                    || loaded_before
                {
                    break;
                }

                loads.push(Self {
                    argument,
                    register,
                    address: item.instruction.address,
                });
            } else if !leaves_esp_and_ebp_alone(insn) {
                // Otherwise it has to be stack setup:
                // `push reg`, `add/sub esp, N` or `mov ebp, esp`.
                match frame.step(insn) {
                    Some(FrameStep::Push(pushed)) if pushed != Register::ESP => {}
                    Some(FrameStep::Adjust | FrameStep::SetBase) => {}
                    _ => break,
                }

                // ESP above starting point means popping caller's stack.
                // Stop there. This function's doing something quite special.
                //
                // Would a sane compiler even emit this??
                if frame.esp > 0 {
                    break;
                }
            }

            // Remember this instruction.
            // We need it for used_earlier above.
            visited_instructions.push(insn);

            // Go to next instruction.
            let Some(next) = stack.successors(index).next() else {
                break;
            };
            index = next;
        }
        loads
    }
}

/// Marks each instruction that more than one path can reach.
///
/// # Returns
///
/// A Vec.
/// Index = Instruction index in `stack.instructions`
/// Value = `true` if more than one path reaches instruction, else `false`.
fn find_insns_with_more_than_one_path(stack: &FunctionStack<'_>) -> Vec<bool> {
    // The entry already has one way in: the call into the function.
    let mut ways_in = vec![0; stack.instructions.len()];
    ways_in[stack.entry] = 1;

    for (index, item) in stack.instructions.iter().enumerate() {
        if item.frame.is_some() {
            for next in stack.successors(index) {
                ways_in[next] += 1;
            }
        }
    }

    ways_in.into_iter().map(|count| count > 1).collect()
}

/// Whether `insn` leaves ESP and EBP alone.
fn leaves_esp_and_ebp_alone(insn: &Insn) -> bool {
    leaves_esp_alone(insn, false) && !RegisterAccess::of(insn).writes(Register::EBP)
}

#[cfg(test)]
mod tests {
    use super::super::function_stack::no_calls;
    use super::*;
    use crate::codegen::optimizations::helpers::tests::analyzed_function;
    use rstest::rstest;

    // ── Core behavior ────────────────────────────────────────────────────────

    /// Each case lists the `(argument, register)` loads found, in the order they run.
    #[rstest]
    #[case::integer(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0
            0xC2, 4, 0,             // ret  4
        ],
        &[(0, Register::EAX)],
    )]
    #[case::sse(
        &[
            0xF3, 0x0F, 0x10, 0x44, 0x24, 4,    // movss xmm0, [esp+4]  ; argument 0
            0xC2, 4, 0,                         // ret   4
        ],
        &[(0, Register::XMM0)],
    )]
    #[case::frame(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x8B, 0x45, 8,          // mov  eax, [ebp+8]    ; argument 0
            0x5D,                   // pop  ebp             ; stop
            0xC2, 4, 0,             // ret  4
        ],
        &[(0, Register::EAX)],
    )]
    #[case::two(
        &[
            0x8B, 0x44, 0x24, 8,    // mov  eax, [esp+8]    ; argument 1
            0x8B, 0x54, 0x24, 4,    // mov  edx, [esp+4]    ; argument 0
            0xC2, 8, 0,             // ret  8
        ],
        &[(1, Register::EAX), (0, Register::EDX)],
    )]
    #[case::after_work(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0
            0x40,                   // inc  eax             ; register work
            0x8B, 0x54, 0x24, 8,    // mov  edx, [esp+8]    ; argument 1
            0xC2, 8, 0,             // ret  8
        ],
        &[(0, Register::EAX), (1, Register::EDX)],
    )]
    #[case::memory_write(
        &[
            0x89, 0x11,             // mov  [ecx], edx      ; stop: writes memory
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]
            0xC2, 4, 0,             // ret  4
        ],
        &[],
    )]
    #[case::saved_later(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0
            0x52,                   // push edx
            0x8B, 0x54, 0x24, 12,   // mov  edx, [esp+12]   ; stop: edx was pushed
            0x5A,                   // pop  edx
            0xC2, 8, 0,             // ret  8
        ],
        &[(0, Register::EAX)],
    )]
    #[case::old_value(
        &[
            0x89, 0xC2,             // mov  edx, eax        ; reads the caller's eax
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; stop: eax was used
            0xC2, 4, 0,             // ret  4
        ],
        &[],
    )]
    #[case::saved_destination(
        &[
            0x50,                   // push eax
            0x8B, 0x44, 0x24, 8,    // mov  eax, [esp+8]    ; stop: eax was pushed
            0x58,                   // pop  eax
            0xC2, 4, 0,             // ret  4
        ],
        &[],
    )]
    #[case::partial_word(
        &[
            0x0F, 0xB6, 0x44, 0x24, 4,  // movzx eax, byte [esp+4]  ; stop: not a whole argument
            0xC2, 4, 0,                 // ret   4
        ],
        &[],
    )]
    #[case::entry_loop(
        &[
            0x8B, 0x44, 0x24, 4,    // .top: mov  eax, [esp+4]  ; stop: two paths reach it
            0x49,                   //       dec  ecx
            0x75, 0xF9,             //       jnz  .top
            0xC2, 4, 0,             //       ret  4
        ],
        &[],
    )]
    fn find_should_return_safe_entry_loads(
        #[case] code: &[u8],
        #[case] expected: &[(u8, Register)],
    ) {
        // Arrange.
        let model = analyzed_function(code);
        let stack = FunctionStack::new(&model, no_calls, false).unwrap();
        let arguments = StackArguments::find(&stack).unwrap();

        // Act.
        let loads = EntryLoad::find_all(&stack, arguments);

        // Assert.
        let actual: Vec<_> = loads
            .iter()
            .map(|load| (load.argument, load.register))
            .collect();
        assert_eq!(actual, expected);
    }
}
