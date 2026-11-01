//! Checks which functions can take their stack arguments in registers.
//!
//! See `eliminate-stack-args.md` for more info.
//!
//! ```asm
//! push esi                ; stack setup
//! mov  eax, [esp+8]       ; entry load: callers put argument 0 in eax instead
//! test ecx, ecx           ; plain work
//! jz   done
//! mov  edx, [esp+8]       ; later load: callers still write argument 0
//! done:
//! pop  esi
//! ret  4
//! ```

use crate::codegen::function::model::{Entry, FunctionModel, Op};
use crate::codegen::optimizations::helpers::call_setup::CallSetup;
use crate::codegen::optimizations::helpers::entry_loads::EntryLoad;
use crate::codegen::optimizations::helpers::function_stack::{
    FunctionStack, StackInstruction, is_branch,
};
use crate::codegen::optimizations::helpers::instruction::plain_work::leaves_esp_alone;
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::optimizations::helpers::stack_arguments::{ArgumentSet, StackArguments};
use crate::codegen::optimizations::helpers::stack_frame::{FrameStep, StackFrame};
use crate::decode::{Insn, MemoryInfo, Mnemonic, OperandAction, Register, memory};

/// A function that can take some of its stack arguments in registers.
pub(super) struct Called {
    /// Its stack arguments.
    pub arguments: StackArguments,

    /// The loads callers do for it, sorted by argument.
    pub loads: Vec<EntryLoad>,

    /// Arguments callers must still write, because something reads them later.
    pub kept: ArgumentSet,
}

impl Called {
    /// Checks whether `model` can take its arguments in registers.
    ///
    /// # Arguments
    ///
    /// - `model`: the function.
    /// - `stack`: its stack positions, from [`FunctionStack::new`].
    ///
    /// # Returns
    ///
    /// The function, or [`None`] if the function has no entry loads, or could
    /// read an argument write we remove.
    pub(super) fn find(model: &FunctionModel, stack: &FunctionStack<'_>) -> Option<Self> {
        // Skip iuncompatible functions.
        if !model.can_change_abi() || model.entry != Entry::First {
            return None;
        }

        // No entry loads means no params. i.e. nothing in registers.
        let arguments = StackArguments::find(stack)?;
        let mut loads = EntryLoad::find_all(stack, arguments);
        if loads.is_empty() {
            return None;
        }
        loads.sort_unstable_by_key(|load| load.argument);

        // For __cdecl, caller may read arguments after the call.
        let mut kept = if arguments.caller_cleanup {
            ArgumentSet::all()
        } else {
            ArgumentSet::empty()
        };

        // Check the rest of the function.
        for item in &stack.instructions {
            // Skip unreachable code.
            let Some(frame) = item.frame else {
                continue;
            };

            let insn = &item.instruction.insn;
            // Does this isn load into register?
            if let Some((argument, _)) = arguments.load(insn, frame, stack.return_slot_bytes) {
                // Is it an entry load? If not, we must preserve.
                let address = item.instruction.address;
                let is_entry_load = loads.iter().any(|load| load.address == address);
                if !is_entry_load {
                    kept |= ArgumentSet::argument(argument);
                }
            } else if !is_safe(item, frame, arguments.caller_cleanup) {
                return None;
            }
        }
        Some(Self {
            arguments,
            loads,
            kept,
        })
    }

    /// Finds the argument writes before one call to this function.
    ///
    /// # Arguments
    ///
    /// - `ops`: the block of ops containing the call, up to the call.
    ///
    /// # Returns
    ///
    /// The setup, or [`None`] if no argument in [`Self::loads`] has a capturable write.
    pub(super) fn call_setup(&self, ops: &[Op]) -> Option<CallSetup> {
        let wanted = self
            .loads
            .iter()
            .map(|load| ArgumentSet::argument(load.argument))
            .collect();
        CallSetup::find(ops, self.arguments, wanted, self.kept)
    }

    /// Removes the entry loads from `model`, since callers now move the
    /// arguments into those registers.
    ///
    /// # Arguments
    ///
    /// - `model`: the function passed to [`Self::find`], after we rewrite its
    ///   own calls.
    ///
    /// # Remarks
    ///
    /// The original entry still does the loads, for callers we didn't rewrite.
    /// See [`FunctionModel::protect_abi`].
    pub(super) fn rewrite(&self, model: &mut FunctionModel) {
        model.protect_abi();
        model.fastcall.register_arguments = self
            .loads
            .iter()
            .map(|load| (load.argument, load.register))
            .collect();

        let is_load = |op: &Op| {
            matches!(op, Op::Guest(guest)
                if self.loads.iter().any(|load| load.address == guest.address))
        };
        for block in &mut model.blocks {
            block.ops.retain(|op| !is_load(op));
        }
    }
}

/// 'If callers stop writing some arguments to the stack, is this instruction still same'
/// Here we find the answer.
fn is_safe(item: &StackInstruction<'_>, mut frame: StackFrame, caller_cleanup: bool) -> bool {
    // Jumps and `ret`: `FunctionStack` already checked them.
    let insn = &item.instruction.insn;
    if is_branch(insn.mnemonic()) || insn.mnemonic() == Mnemonic::RET {
        return true;
    }

    // The function we call could read a removed argument write.
    if item.call_bytes.is_some() {
        return caller_cleanup;
    }

    let safe = match frame.step(insn) {
        // Like `add eax, ecx` or `mov [ecx], eax`.
        Some(FrameStep::Unchanged) => is_plain_work(insn, caller_cleanup),

        // `push esp` hands out a stack address.
        Some(FrameStep::Push(Register::ESP)) => false,

        // `push [mem]`
        Some(FrameStep::Adjust) => {
            memory(&insn.operands()[0]).is_none_or(|mem| caller_cleanup && is_plain_memory(mem))
        }

        // Other stack moves, like `push esi`, `pop esi` or `leave`.
        Some(_) => true,
        None => false,
    };

    // Never pop into the caller's stack/buffer
    safe && frame.esp <= 0
}

/// Whether `mem` is DS or SS memory, not addressed through ESP or EBP.
fn is_plain_memory(mem: &MemoryInfo) -> bool {
    let stack = |reg| matches!(reg, Register::ESP | Register::EBP);
    matches!(mem.segment, Register::DS | Register::SS) && !stack(mem.base) && !stack(mem.index)
}

/// Whether `insn` is plain work that can't read a removed argument write.
fn is_plain_work(insn: &Insn, allow_reads: bool) -> bool {
    let stores_allowed = true;
    let access = RegisterAccess::of(insn);
    leaves_esp_alone(insn, stores_allowed)
        && !access.touches(Register::ESP)
        && !access.touches(Register::EBP)
        && (allow_reads || !reads_memory(insn))
}

/// Whether `insn` reads memory.
fn reads_memory(insn: &Insn) -> bool {
    // `lea` only computes an address.
    if insn.mnemonic() == Mnemonic::LEA {
        return false;
    }

    // Skip hidden operands, like the stack slot `push` writes.
    let visible = usize::from(insn.instruction().operand_count_visible);
    let operands = &insn.operands()[..visible];
    let reads = OperandAction::READ | OperandAction::CONDREAD;
    operands
        .iter()
        .any(|operand| memory(operand).is_some() && operand.action.intersects(reads))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::function_stack::CallEffect;
    use crate::codegen::optimizations::helpers::tests::analyzed_function;
    use crate::config::FunctionAddress;
    use rstest::rstest;

    /// Runs [`Called::find`] on `code`.
    ///
    /// `code` may call 0x1040, which ends with a plain `ret`.
    fn find(code: &[u8]) -> Option<Called> {
        let model = analyzed_function(code);
        let stack = FunctionStack::new(&model, |target| {
            (target == FunctionAddress(0x1040)).then_some(CallEffect {
                popped: 4,
                keeps_ebp: true,
            })
        })
        .expect("every case is a function we can follow");
        Called::find(&model, &stack)
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    /// `ret N` means the called function removes its arguments. A plain `ret` means the
    /// caller does, and keeps every argument write.
    #[rstest]
    #[case::frame(
        &[
            0x55,                       // push ebp
            0x89, 0xE5,                 // mov  ebp, esp
            0x8B, 0x45, 8,              // mov  eax, [ebp+8]    ; entry load
            0x5D,                       // pop  ebp
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::locals(
        &[
            0x83, 0xEC, 8,              // sub  esp, 8
            0x8B, 0x44, 0x24, 12,       // mov  eax, [esp+12]   ; entry load
            0x83, 0xC4, 8,              // add  esp, 8
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::branch(
        &[
            0x8B, 0x44, 0x24, 4,        //     mov  eax, [esp+4]
            0x85, 0xC0,                 //     test eax, eax
            0x74, 1,                    //     jz   .done
            0x40,                       //     inc  eax
            0xC2, 4, 0,                 // .done: ret 4
        ],
    )]
    #[case::store(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x89, 0x01,                 // mov  [ecx], eax      ; stores are fine
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::vector_store(
        &[
            0x8B, 0x44, 0x24, 4,        // mov    eax, [esp+4]
            0x0F, 0x29, 0x01,           // movaps [ecx], xmm0   ; stores are fine
            0xC2, 4, 0,                 // ret    4
        ],
    )]
    #[case::cdecl_read(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x8B, 0x00,                 // mov  eax, [eax]      ; callers keep every write
            0xC3,                       // ret
        ],
    )]
    #[case::cdecl_push_memory(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0xFF, 0x31,                 // push [ecx]           ; callers keep every write
            0x5A,                       // pop  edx
            0xC3,                       // ret
        ],
    )]
    #[case::cdecl_call(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0xE8, 0x37, 0, 0, 0,        // call 0x1040          ; callers keep every write
            0xC3,                       // ret
        ],
    )]
    fn find_should_accept_safe_functions(#[case] code: &[u8]) {
        assert!(find(code).is_some());
    }

    /// Each case could read a removed argument write, or does something we
    /// don't follow.
    #[rstest]
    #[case::no_entry_loads(
        &[
            0xC2, 4, 0,                 // ret  4               ; <- nothing to pass in registers
        ],
    )]
    #[case::read(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x8B, 0x00,                 // mov  eax, [eax]      ; <- might read a removed write
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::read_and_write(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x01, 0x01,                 // add  [ecx], eax      ; <- reads [ecx] too
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::push_memory(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0xFF, 0x31,                 // push [ecx]           ; <- might read a removed write
            0x5A,                       // pop  edx
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::call(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0xE8, 0x37, 0, 0, 0,        // call 0x1040          ; <- might read a removed write
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::cdecl_stack_read(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x66, 0x8B, 0x4C, 0x24, 4,  // mov  cx, [esp+4]     ; <- through ESP
            0xC3,                       // ret
        ],
    )]
    #[case::cdecl_push_stack(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0xFF, 0x74, 0x24, 4,        // push [esp+4]         ; <- through ESP
            0x5A,                       // pop  edx
            0xC3,                       // ret
        ],
    )]
    #[case::cdecl_push_esp(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x54,                       // push esp             ; <- hands out a stack address
            0x59,                       // pop  ecx
            0xC3,                       // ret
        ],
    )]
    #[case::pop_return_address(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]
            0x59,                       // pop  ecx             ; <- pops the caller's stack
            0x51,                       // push ecx
            0xC2, 4, 0,                 // ret  4
        ],
    )]
    #[case::unknown_instruction(
        &[
            0x8B, 0x44, 0x24, 4,        // mov   eax, [esp+4]
            0x0F, 0xC8,                 // bswap eax            ; <- not plain work we know
            0xC2, 4, 0,                 // ret   4
        ],
    )]
    fn find_should_reject_unsafe_functions(#[case] code: &[u8]) {
        assert!(find(code).is_none());
    }

    /// Callers must keep an argument write when something reads it after the entry.
    #[rstest]
    #[case::entry_load_only(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]    ; entry load
            0xC2, 4, 0,                 // ret  4
        ],
        ArgumentSet::empty(),
    )]
    #[case::later_load(
        &[
            0x8B, 0x44, 0x24, 4,        //     mov  eax, [esp+4]    ; entry load
            0x85, 0xC9,                 //     test ecx, ecx
            0x74, 4,                    //     jz   .done
            0x8B, 0x54, 0x24, 4,        //     mov  edx, [esp+4]    ; later load of argument 0
            0xC2, 4, 0,                 // .done: ret 4
        ],
        ArgumentSet::ARG0,
    )]
    #[case::register_saved_first(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]    ; entry load
            0x52,                       // push edx
            0x8B, 0x54, 0x24, 12,       // mov  edx, [esp+12]   ; later load: edx was pushed
            0x5A,                       // pop  edx
            0xC2, 8, 0,                 // ret  8
        ],
        ArgumentSet::ARG1,
    )]
    #[case::load_into_ebp(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]    ; entry load
            0x8B, 0x6C, 0x24, 8,        // mov  ebp, [esp+8]    ; later load of argument 1
            0xC2, 8, 0,                 // ret  8
        ],
        ArgumentSet::ARG1,
    )]
    #[case::caller_cleanup(
        &[
            0x8B, 0x44, 0x24, 4,        // mov  eax, [esp+4]    ; entry load
            0xC3,                       // ret                  ; caller may read them after
        ],
        ArgumentSet::all(),
    )]
    fn find_should_keep_writes_read_later(#[case] code: &[u8], #[case] expected: ArgumentSet) {
        // Act
        let function = find(code).unwrap();

        // Assert
        assert_eq!(function.kept, expected);
    }
}
