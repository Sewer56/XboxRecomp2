//! Check the extra instructions between `cmp` and the table `jmp`.
//!
//! A gap is an instruction between the comparison, branch, and table jump:
//!
//! ```text
//! cmp eax, 3
//! mov edx, 5             ; gap before the branch
//! ja  default_case
//! inc ebx                ; gap after the branch
//! jmp [eax*4 + targets]
//! ```
//!
//! We check three things separately:
//!
//! * [`preserves_index_path`]: does the instruction leave the checked registers
//!   alone and stay on this path?
//! * [`flag_writes`]: which flags might change before `ja`/`jae` reads them?
//! * [`writes_memory`]: could a store change the bytes used by a later lookup?
//!
//! The caller handles `ja`/`jae` and the allowed index copy or byte lookup.
//! See the [parent module] for those rules.
//!
//! [parent module]: super

use crate::decode::{
    BranchType, CpuFlag, Insn, InstructionAttributes, InstructionCategory, MachineMode, Mnemonic,
    OperandAction, Register, memory, register,
};

/// Reports which CPU flags this instruction might change.
///
/// Flags hold the comparison result used by `ja`/`jae`.
/// The caller checks whether any required flag changes after `cmp`.
///
/// # Arguments
///
/// * `insn` - the instruction between `cmp` and `ja`/`jae`.
///
/// # Returns
///
/// Flags the instruction modifies, sets, clears, or leaves undefined.
/// An empty set means it leaves all flags alone.
pub(super) fn flag_writes(insn: &Insn) -> CpuFlag {
    let flags = insn.instruction().cpu_flags;
    flags.modified | flags.set_0 | flags.set_1 | flags.undefined
}

/// Checks whether an extra instruction leaves the input, index, and path intact.
///
/// For `cmp eax, 3` and `jmp [esi*4 + targets]`, `mov edx, 5` is OK.
/// `inc eax` changes the input, while `call helper` leaves the path we can follow.
///
/// # Arguments
///
/// * `insn` - the extra instruction to check.
/// * `compared` - the register checked by `cmp`: EAX above.
/// * `index` - the register used by the table jump: ESI above.
///
/// # Remarks
///
/// A `true` result alone does not make the instruction safe to ignore.
/// The caller checks flags with [`flag_writes`] and stores with [`writes_memory`].
///
/// # Returns
///
/// `true` if the instruction passes these checks, or `false` if it:
///
/// * Calls, branches, returns, traps, or starts or aborts a transaction.
/// * Is invalid, halts the CPU, or performs a system or I/O operation.
/// * Needs privileged access or restores CPU, floating-point, or vector state.
/// * Writes either checked register, including partial or conditional writes.
/// * Writes a segment register or the instruction pointer, EIP.
pub(super) fn preserves_index_path(insn: &Insn, compared: Register, index: Register) -> bool {
    let instruction = insn.instruction();
    if instruction.meta.branch_type != BranchType::NONE
        || matches!(
            instruction.meta.category,
            InstructionCategory::INVALID
                | InstructionCategory::CALL
                | InstructionCategory::COND_BR
                | InstructionCategory::UNCOND_BR
                | InstructionCategory::RET
                | InstructionCategory::INTERRUPT
                | InstructionCategory::SYSCALL
                | InstructionCategory::SYSRET
                | InstructionCategory::SYSTEM
                | InstructionCategory::IO
                | InstructionCategory::IOSTRINGOP
        )
        || instruction.attributes.intersects(
            InstructionAttributes::IS_PRIVILEGED
                | InstructionAttributes::CPU_STATE_CW
                | InstructionAttributes::FPU_STATE_CW
                | InstructionAttributes::XMM_STATE_CW,
        )
        || matches!(
            insn.mnemonic(),
            Mnemonic::UD0
                | Mnemonic::UD1
                | Mnemonic::UD2
                | Mnemonic::HLT
                | Mnemonic::XBEGIN
                | Mnemonic::XABORT
        )
    {
        return false;
    }

    // Writes are not always obvious from the destination operand:
    //
    // * `mul ebx` also writes EAX and EDX.
    // * `cmovz eax, ebx` may write EAX, depending on the flags.
    // * `mov al, 2` changes part of EAX.
    insn.operands().iter().all(|operand| {
        if !operand
            .action
            .intersects(OperandAction::WRITE | OperandAction::CONDWRITE)
        {
            return true;
        }
        let Some(reg) = register(operand) else {
            return true;
        };

        // Treat AL, AH, and AX writes as EAX writes too.
        let parent = reg.largest_enclosing(MachineMode::LEGACY_32);

        // Segment changes can alter table addresses.
        // Writing EIP changes which instruction runs next.
        parent != compared
            && parent != index
            && !matches!(
                reg,
                Register::CS
                    | Register::DS
                    | Register::ES
                    | Register::FS
                    | Register::GS
                    | Register::SS
                    | Register::EIP
            )
    })
}

/// Checks whether this instruction might write to memory.
///
/// We include ordinary stores and less obvious ones:
///
/// * `mov [remap], dl` writes a byte.
/// * `push ebx` writes to the stack.
/// * `cmpxchg [remap], ecx` may write depending on the comparison result.
///
/// # Arguments
///
/// * `insn` - the instruction to check for stores.
///
/// # Remarks
///
/// We do not check which address a store reaches. Any earlier memory write
/// in the sequence makes the caller reject a later byte lookup.
///
/// # Returns
///
/// `true` if the instruction might write to memory, even implicitly or only
/// under a condition. `false` for reads alone or no memory access.
pub(super) fn writes_memory(insn: &Insn) -> bool {
    insn.operands().iter().any(|operand| {
        operand
            .action
            .intersects(OperandAction::WRITE | OperandAction::CONDWRITE)
            && memory(operand).is_some()
    })
}
