//! Instructions that copy a value into a register.

use super::stack_range::StackRange;
use crate::codegen::optimizations::helpers::instruction::general_registers::GeneralRegisters;
use crate::codegen::optimizations::helpers::instruction::stack_slot::is_stack_slot;
use crate::decode::{Insn, Mnemonic, immediate, memory, register};

/// An instruction that sets a register to the same value on every pass, as long
/// as its sources don't change.
///
/// ```text
/// mov eax, 5              ->  { target: EAX, sources: {},        stack_source: None }
/// mov eax, ebx            ->  { target: EAX, sources: EBX,       stack_source: None }
/// lea eax, [ebx+ecx*4+8]  ->  { target: EAX, sources: EBX | ECX, stack_source: None }
/// mov eax, [esp+8]        ->  { target: EAX, sources: ESP,       stack_source: esp+8..esp+12 }
/// ```
#[derive(Clone, Copy)]
pub(super) struct RegisterCopy {
    /// The register it writes.
    pub target: GeneralRegisters,

    /// The registers it reads.
    pub sources: GeneralRegisters,

    /// The stack bytes it reads, if we know which.
    pub stack_source: Option<StackRange>,
}

impl RegisterCopy {
    /// Reads `insn` as a copy.
    ///
    /// # Returns
    ///
    /// [`None`] if `insn` isn't a copy.
    pub(super) fn of(insn: &Insn, ebp_frame: bool) -> Option<Self> {
        let is_lea = match insn.mnemonic() {
            Mnemonic::MOV => false,
            Mnemonic::LEA => true,
            _ => return None,
        };

        // Skip partial writes: `mov al, 1` keeps the rest of EAX.
        let operands = insn.operands();
        let target = register(&operands[0]).map(GeneralRegisters::of)?;
        if target.is_empty() || operands[0].size != 32 {
            return None;
        }

        let source = &operands[1];
        let (sources, stack_source) = match memory(source) {
            // lea eax, [ebx+ecx*4+8]
            Some(mem) if is_lea => (GeneralRegisters::in_address(mem), None),

            // mov eax, [esp+8]
            Some(mem) if is_stack_slot(mem, ebp_frame) => (
                GeneralRegisters::in_address(mem),
                StackRange::of(mem, source.size),
            ),

            // mov eax, [ebx]: no good, another thread can change it.
            Some(_) => return None,

            // mov eax, ebx
            None => match register(source) {
                Some(reg) => (GeneralRegisters::of(reg), None),

                // mov eax, 5
                None => match immediate(source) {
                    Some(_) => (GeneralRegisters::empty(), None),
                    None => return None,
                },
            },
        };

        Some(Self {
            target,
            sources,
            stack_source,
        })
    }
}
