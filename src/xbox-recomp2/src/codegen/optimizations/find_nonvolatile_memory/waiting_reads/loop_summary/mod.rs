//! Sums up what one loop does with registers and memory.

use crate::codegen::optimizations::helpers::instruction::control_transfer::leaves_block;
use crate::codegen::optimizations::helpers::instruction::general_registers::GeneralRegisters;
use crate::codegen::optimizations::helpers::instruction::stack_slot::is_stack_slot;
use crate::codegen::optimizations::helpers::op_graph::OpIndex;
use crate::decode::{
    DecodedOperand, Insn, InstructionCategory, MemoryInfo, OperandAction, memory, register,
};
use register_copy::RegisterCopy;
use stack_range::StackRange;

mod register_copy;
mod stack_range;

/// What one loop does with registers and memory.
///
/// For each loop:
///
/// - [`clear`](Self::clear)
/// - [`add`](Self::add) each instruction in the loop
/// - [`mark_reads_at_fixed_address`](Self::mark_reads_at_fixed_address)
#[derive(Default)]
pub(super) struct LoopSummary {
    /// Whether SSE loads skip `volatile` already, so no wait can rely on them.
    pub nonvolatile_sse: bool,

    /// Registers the loop writes.
    written: GeneralRegisters,

    /// Registers the loop writes with something other than a [`RegisterCopy`].
    written_by_non_copies: GeneralRegisters,

    /// The register copies in the loop.
    copies: Vec<RegisterCopy>,

    /// The stack bytes the loop stores to.
    stack_stores: Vec<StackRange>,

    /// The memory reads outside the stack.
    reads: Vec<Read>,
}

/// A memory read outside the stack.
#[derive(Clone, Copy)]
struct Read {
    /// The registers its address is built from.
    address: GeneralRegisters,

    /// The op that reads.
    op: u32,
}

impl LoopSummary {
    /// Forgets the previous loop.
    pub(super) fn clear(&mut self) {
        self.written = GeneralRegisters::empty();
        self.written_by_non_copies = GeneralRegisters::empty();
        self.copies.clear();
        self.stack_stores.clear();
        self.reads.clear();
    }

    /// Adds one instruction of the loop.
    ///
    /// # Arguments
    ///
    /// - `op`: the op that holds `insn`.
    /// - `insn`: the instruction.
    /// - `ebp_frame`: whether EBP is the function's frame pointer.
    pub(super) fn add(&mut self, op: u32, insn: &Insn, ebp_frame: bool) {
        // Note the registers and memory it touches.
        let count = usize::from(insn.instruction().operand_count);
        let reads_data = reads_guest_data(insn);
        let mut written = GeneralRegisters::empty();

        for operand in &insn.operands()[..count] {
            match memory(operand) {
                Some(mem) => self.add_memory(op, operand, mem, reads_data, ebp_frame),
                None => written |= written_register(operand),
            }
        }

        self.written |= written;
        match RegisterCopy::of(insn, ebp_frame) {
            Some(copy) => self.copies.push(copy),
            None => self.written_by_non_copies |= written,
        }
    }

    /// Adds one memory operand of the loop.
    fn add_memory(
        &mut self,
        op: u32,
        operand: &DecodedOperand,
        mem: &MemoryInfo,
        reads_data: bool,
        ebp_frame: bool,
    ) {
        // Is this an esp/ebp write?
        if is_stack_slot(mem, ebp_frame) {
            if writes(operand) {
                self.stack_stores.extend(StackRange::of(mem, operand.size));
            }

            return;
        }

        // Not a data read? Skip!
        if !reads_data || !reads(operand) {
            return;
        }

        // SSE loads that skip `volatile` already.
        if self.nonvolatile_sse && operand.size == 128 {
            return;
        }

        self.reads.push(Read {
            address: GeneralRegisters::in_address(mem),
            op,
        });
    }

    /// Marks the reads whose address is the same on every pass.
    ///
    /// ```text
    /// loop: mov eax, [esi]  ; marked: the loop never changes ESI
    ///       mov ecx, [edi]  ; not marked: EDI changes
    ///       add edi, 4
    ///       jmp loop
    /// ```
    ///
    /// # Arguments
    ///
    /// - `waiting_reads`: set to `true` for each op with a marked read.
    pub(super) fn mark_reads_at_fixed_address(&self, waiting_reads: &mut [bool]) {
        if self.reads.is_empty() {
            return;
        }

        // An address is fixed if none of its registers change.
        let changing = self.written - self.registers_that_keep_value();
        for read in &self.reads {
            if !read.address.intersects(changing) {
                waiting_reads[read.op as OpIndex] = true;
            }
        }
    }

    /// The registers the loop writes, but with the same value on every pass.
    ///
    /// ```text
    /// loop: mov eax, [esp+8]  ; kept: the loop never writes ESP or [esp+8]
    ///       mov ecx, eax      ; kept: EAX is computer from above
    ///       mov edx, 5
    ///       shl edx, 2        ; not kept: `shl` isn't a copy
    ///       mov ebx, edx      ; not kept: EDX isn't kept
    ///       jmp loop
    /// ```
    fn registers_that_keep_value(&self) -> GeneralRegisters {
        // Repeat until nothing changes: ECX is only kept once EAX is.
        let mut last_written = GeneralRegisters::empty();

        loop {
            // Registers with the same value on every pass.
            let steady = !self.written | last_written;

            // Keep each copy whose sources are steady.
            let mut next = self.written - self.written_by_non_copies;
            for copy in &self.copies {
                if !steady.contains(copy.sources) || self.stores_to(copy.stack_source) {
                    next.remove(copy.target);
                }
            }

            if next == last_written {
                return last_written;
            }

            last_written = next;
        }
    }

    /// Whether the loop stores to any byte of `range`.
    fn stores_to(&self, range: Option<StackRange>) -> bool {
        let range = match range {
            Some(range) => range,
            None => return false,
        };

        self.stack_stores.iter().any(|store| store.overlaps(range))
    }
}

/// Whether `insn`'s memory operands read guest data.
///
/// i.e. Read somewhere from the heap.
/// But not read-only data, like switch table.
fn reads_guest_data(insn: &Insn) -> bool {
    let category = insn.instruction().meta.category;

    !leaves_block(insn)
        && !matches!(
            category,
            InstructionCategory::NOP | InstructionCategory::WIDENOP | InstructionCategory::PREFETCH
        )
}

/// The register `operand` writes, if any.
fn written_register(operand: &DecodedOperand) -> GeneralRegisters {
    match register(operand) {
        Some(reg) if writes(operand) => GeneralRegisters::of(reg),
        _ => GeneralRegisters::empty(),
    }
}

/// Whether `operand` may be written.
fn writes(operand: &DecodedOperand) -> bool {
    operand
        .action
        .intersects(OperandAction::WRITE | OperandAction::CONDWRITE)
}

/// Whether `operand` may be read.
fn reads(operand: &DecodedOperand) -> bool {
    operand
        .action
        .intersects(OperandAction::READ | OperandAction::CONDREAD)
}
