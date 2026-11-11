//! The `stack_slots_in_locals` speed hack. See `stack-slots-in-locals.md`.
//!
//! Moves a leaf function's stack slots into a local `frame` array, if:
//!
//! - It has no `call`, hook or failed decode.
//! - We can track ESP on every path.
//! - No stack address escapes, like `lea ecx, [esp+4]`.
//! - Every slot below the entry ESP is a load, store or `_MEM` helper we can
//!   point at `frame`.

mod slot_use;
mod slots;

use super::helpers::function_stack::{FunctionStack, StackInstruction, no_calls};
use super::helpers::instruction::register_access::RegisterAccess;
use super::helpers::stack_frame::{FrameStep, STACK_WORD_BYTES, StackFrame};
use crate::codegen::function::model::{FunctionModel, Op, StackLocals};
use crate::decode::{
    DecodedOperand, Insn, InstructionAttributes, MemoryInfo, Mnemonic, OperandAction,
    OperandVisibility, Register, memory, register,
};
use slot_use::SlotUse;
use slots::{OperandIndex, Slots};
use std::collections::BTreeMap;
use std::io::Write;

/// Max `frame` size, in bytes.
const MAX_FRAME_BYTES: i32 = 0x1000;

/// Offset from the entry ESP.
type EspOffset = i32;

/// Sets [`FunctionModel::stack_locals`] on every function that qualifies.
pub(super) fn run(models: &mut [FunctionModel], diag: &mut dyn Write) {
    let mut count = 0usize;
    for model in models.iter_mut() {
        model.stack_locals = make_plan_for_model(model);
        if model.stack_locals.is_some() {
            count += 1;
        }
    }

    let _ = writeln!(diag, "stack-slots-in-locals: {count} functions");
}

fn make_plan_for_model(model: &FunctionModel) -> Option<StackLocals> {
    // Leaf functions only.
    if !is_leaf_function(model) {
        return None;
    }

    let stack = FunctionStack::new(model, no_calls, true)?;

    // Collect each instruction's stack slots.
    let mut used = Vec::new();
    let mut lowest: EspOffset = 0;
    for item in stack.reachable() {
        let slots = stack_slots_used_by(item)?;
        if slots.is_empty() {
            continue;
        }

        lowest = lowest.min(slots.lowest());
        used.push((item.instruction.address, slots));
    }

    // If no stack, or above max frame size, skip.
    if used.is_empty() || lowest < -MAX_FRAME_BYTES {
        return None;
    }

    // Align 16. Needed for SSE on x86 hosts. (yes, dumb)
    let size = (-lowest + 15) & !15;
    let mut instructions = BTreeMap::new();
    for (address, slots) in used {
        instructions.insert(address, slots.into_frame_access(size));
    }

    Some(StackLocals {
        size: size as u32,
        instructions,
    })
}

/// Whether this is a leaf function we can process.
fn is_leaf_function(model: &FunctionModel) -> bool {
    let mut ops = model.blocks.iter().flat_map(|block| &block.ops);
    !ops.any(|op| matches!(op, Op::Call(_) | Op::Failed { .. } | Op::MergedCmpJmp(_)))
}

/// Stack slots used by `item`, or [`None`] if:
/// - it leaks a stack address
/// - it uses a slot we can't rewrite
fn stack_slots_used_by(item: &StackInstruction<'_>) -> Option<Slots> {
    let frame = item.frame?;
    let insn = &item.instruction.insn;

    // If stack leaks elsewhere we can't track, bail out.
    if leaks_stack_address(insn, frame) {
        return None;
    }

    // `ret` reads nothing below the entry ESP.
    let mut slots = Slots::default();
    if item.return_op.is_none() {
        match push_pop_slot(insn, frame) {
            SlotUse::Outside => {}
            SlotUse::Local(offset) => slots.push_pop = Some(offset),
            SlotUse::Unsupported => return None,
        }
    }

    let count = usize::from(insn.instruction().operand_count);
    for (index, operand) in insn.operands()[..count].iter().enumerate() {
        // Hidden operands are the push/pop slot.
        if operand.visibility == OperandVisibility::HIDDEN {
            continue;
        }

        match operand_slot(insn, operand, frame) {
            SlotUse::Outside => {}
            SlotUse::Local(offset) => slots.operands.push((index as OperandIndex, offset)),
            SlotUse::Unsupported => return None,
        }
    }

    Some(slots)
}

/// Whether `insn` leaks a stack address, like `lea ecx, [esp+4]`.
fn leaks_stack_address(insn: &Insn, frame: StackFrame) -> bool {
    let operands = insn.operands();
    // Moving ESP itself is fine, like `lea esp, [esp+8]`.
    if StackFrame::adjustment(insn).is_some() || copies_esp_or_ebp(insn) {
        return false;
    }

    match insn.mnemonic() {
        Mnemonic::LEA => memory(&operands[1]).is_some_and(|mem| uses_stack_register(mem, frame)),

        Mnemonic::PUSH => register(&operands[0]).is_some_and(|reg| points_into_stack(reg, frame)),

        Mnemonic::POP | Mnemonic::RET | Mnemonic::LEAVE => false,

        _ => {
            let access = RegisterAccess::of(insn);
            access.reads_value(Register::ESP)
                || (frame.ebp.is_some() && access.reads_value(Register::EBP))
        }
    }
}

/// Slot of a `push`, `pop` or `leave`.
fn push_pop_slot(insn: &Insn, frame: StackFrame) -> SlotUse {
    let word = STACK_WORD_BYTES as i32;
    let mut after = frame;
    let offset = match after.step(insn) {
        Some(FrameStep::Push(_)) => after.esp,
        Some(FrameStep::Adjust) if insn.mnemonic() == Mnemonic::PUSH => after.esp,

        // `leave` pops EBP.
        Some(FrameStep::Pop(_)) => after.esp - word,
        Some(FrameStep::Restore) if insn.mnemonic() == Mnemonic::LEAVE => after.esp - word,

        Some(_) => return SlotUse::Outside,
        None => return SlotUse::Unsupported,
    };

    // `X86_FRAME_PUSH`/`POP` are 32-bit only.
    if insn.instruction().operand_width != 32 {
        return SlotUse::Unsupported;
    }

    // `pop [mem]`: rare, not worth handling.
    if insn.mnemonic() == Mnemonic::POP && register(&insn.operands()[0]).is_none() {
        return SlotUse::Unsupported;
    }

    if offset >= 0 {
        return SlotUse::Outside;
    }

    SlotUse::Local(offset)
}

/// Slot of a visible memory operand.
fn operand_slot(insn: &Insn, operand: &DecodedOperand, frame: StackFrame) -> SlotUse {
    // `lea` only computes an address. `leaks_stack_address` checks it.
    if insn.mnemonic() == Mnemonic::LEA {
        return SlotUse::Outside;
    }

    let mem = match memory(operand) {
        Some(mem) => mem,
        None => return SlotUse::Outside,
    };
    if !uses_stack_register(mem, frame) {
        return SlotUse::Outside;
    }

    // `[esp+eax*4]` could be any slot.
    if mem.index != Register::NONE {
        return SlotUse::Unsupported;
    }

    let start = match frame.address(mem) {
        Some(start) => start,
        None => return SlotUse::Unsupported,
    };
    if start >= 0 {
        return SlotUse::Outside;
    }

    // Must fit below the entry ESP and be a plain load or store.
    let bytes = i32::from(operand.size.div_ceil(8));
    if start + bytes > 0 || !can_rewrite_operand(insn, operand.size, operand.action) {
        return SlotUse::Unsupported;
    }

    SlotUse::Local(start)
}

/// e.g.
/// - `[esp+4]`
/// - `[ebp-8]` when EBP is tracked
/// - `[eax+ebp*2]` when EBP is tracked
fn uses_stack_register(mem: &MemoryInfo, frame: StackFrame) -> bool {
    points_into_stack(mem.base, frame) || points_into_stack(mem.index, frame)
}

/// ESP, or EBP when tracked.
///
/// e.g.
/// - `esp`
/// - `ebp` after `mov ebp, esp`
fn points_into_stack(reg: Register, frame: StackFrame) -> bool {
    reg == Register::ESP || (reg == Register::EBP && frame.ebp.is_some())
}

/// e.g.
/// - `mov ebp, esp`
/// - `mov esp, ebp`
fn copies_esp_or_ebp(insn: &Insn) -> bool {
    let operands = insn.operands();

    insn.mnemonic() == Mnemonic::MOV
        && matches!(
            (register(&operands[0]), register(&operands[1])),
            (Some(Register::EBP), Some(Register::ESP)) | (Some(Register::ESP), Some(Register::EBP))
        )
}

/// Whether the operand is emitted as `X86_LOAD_*`, `X86_STORE_*` or a `_MEM` helper.
/// i.e. Code we can rewrite.
fn can_rewrite_operand(insn: &Insn, bits: u16, action: OperandAction) -> bool {
    if !matches!(bits, 8 | 16 | 32 | 64 | 128) {
        return false;
    }

    let reads = action.intersects(OperandAction::READ | OperandAction::CONDREAD);
    let writes = action.intersects(OperandAction::WRITE | OperandAction::CONDWRITE);
    match (reads, writes) {
        (true, true) => modifies_with_helper(insn),
        (false, true) => stores_with_macro(insn.mnemonic()),
        _ => loads_with_macro(insn.mnemonic()),
    }
}

/// Whether we can rewrite this read-modify-write.
///
/// e.g. `and [esp+4], 0xFF` becomes `X86_AND32_MEM(ctx.esp.u32 + 4, 0xff, ctx, base)`.
fn modifies_with_helper(insn: &Insn) -> bool {
    match insn.mnemonic() {
        // `lock xadd` is atomic, with its own helper.
        Mnemonic::XADD => !insn
            .instruction()
            .attributes
            .contains(InstructionAttributes::HAS_LOCK),

        mnemonic => matches!(
            mnemonic,
            Mnemonic::ADD
                | Mnemonic::ADC
                | Mnemonic::SUB
                | Mnemonic::SBB
                | Mnemonic::AND
                | Mnemonic::OR
                | Mnemonic::XOR
                | Mnemonic::INC
                | Mnemonic::DEC
                | Mnemonic::NEG
                | Mnemonic::NOT
                | Mnemonic::SHL
                | Mnemonic::SHR
                | Mnemonic::SAR
                | Mnemonic::ROL
                | Mnemonic::ROR
                | Mnemonic::RCR
                | Mnemonic::SHLD
                | Mnemonic::SHRD
        ),
    }
}

/// Whether the emitter writes this store as `X86_STORE_*`.
///
/// e.g. `mov [esp+4], eax` becomes `X86_STORE_U32(ctx.esp.u32 + 4, ctx.eax.u32)`.
///
/// A function with an unlisted store to a local slot is skipped.
fn stores_with_macro(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::MOV
            | Mnemonic::MOVSS
            | Mnemonic::MOVAPS
            | Mnemonic::MOVUPS
            | Mnemonic::MOVLPS
            | Mnemonic::MOVHPS
            | Mnemonic::MOVD
            | Mnemonic::MOVQ
            | Mnemonic::FST
            | Mnemonic::FSTP
            | Mnemonic::FISTP
            | Mnemonic::FNSTCW
            | Mnemonic::FNSTSW
            | Mnemonic::STMXCSR
            | Mnemonic::SETB
            | Mnemonic::SETBE
            | Mnemonic::SETL
            | Mnemonic::SETLE
            | Mnemonic::SETNB
            | Mnemonic::SETNBE
            | Mnemonic::SETNL
            | Mnemonic::SETNLE
            | Mnemonic::SETNO
            | Mnemonic::SETNP
            | Mnemonic::SETNS
            | Mnemonic::SETNZ
            | Mnemonic::SETO
            | Mnemonic::SETP
            | Mnemonic::SETS
            | Mnemonic::SETZ
    )
}

/// Whether the emitter reads this operand with `X86_LOAD_*`, or something we can rewrite.
///
/// e.g.
/// - `mov eax, [esp+4]`: yes, `X86_LOAD_U32(ctx.esp.u32 + 4)`.
/// - `paddw xmm0, [esp]`: yes, `base + (ctx.esp.u32)` becomes `frame + 0x10`.
/// - `cvttss2si eax, [esp]`: yes, `X86_CVTSS2SI_MEM` is a `_MEM` helper.
/// - `frstor [esp]`: no, has its own helper.
///
/// Most do, so we list the ones that don't.
fn loads_with_macro(mnemonic: Mnemonic) -> bool {
    !matches!(
        mnemonic,
        Mnemonic::MOVNTPS
            | Mnemonic::MOVNTQ
            | Mnemonic::XCHG
            | Mnemonic::CMPXCHG
            | Mnemonic::XADD
            | Mnemonic::FNSAVE
            | Mnemonic::FRSTOR
            | Mnemonic::CALL
            | Mnemonic::JMP
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::analyzed_function;
    use rstest::rstest;

    /// `(address, operand offsets, push/pop offset)` per instruction.
    fn frame_offsets(locals: &StackLocals) -> Vec<(u32, Vec<u32>, Option<u32>)> {
        let entries = locals.instructions.iter();

        entries
            .map(|(address, access)| {
                let operands = access.operands.iter().map(|&(_, offset)| offset).collect();
                (address.get(), operands, access.push_pop)
            })
            .collect()
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    /// Locals move to `frame`; arguments stay.
    ///
    /// ```text
    ///  +4   argument 0      <- [esp+8] after the push: stays in guest memory
    ///  +0   return address
    ///  -4   saved esi       <- frame[0xC]
    ///  -8   local           <- frame[0x8], [esp] after `sub esp, 4`
    /// ```
    #[test]
    fn plan_should_map_slots_below_entry_esp() {
        // Arrange
        let model = analyzed_function(&[
            0x56, //                   1000: push esi
            0x8B, 0x44, 0x24, 0x08, // 1001: mov  eax, [esp+8]  ; argument 0
            0x83, 0xEC, 0x04, //       1005: sub  esp, 4
            0x89, 0x04, 0x24, //       1008: mov  [esp], eax    ; local
            0x8B, 0x0C, 0x24, //       100B: mov  ecx, [esp]
            0x83, 0xC4, 0x04, //       100E: add  esp, 4
            0x5E, //                   1011: pop  esi
            0xC3, //                   1012: ret
        ]);

        // Act
        let locals = make_plan_for_model(&model).unwrap();

        // Assert
        assert_eq!(locals.size, 0x10);
        assert_eq!(
            frame_offsets(&locals),
            [
                (0x1000, vec![], Some(0xC)),
                (0x1008, vec![0x8], None),
                (0x100B, vec![0x8], None),
                (0x1011, vec![], Some(0xC)),
            ]
        );
    }

    /// `[ebp-N]` and `leave` use the slots EBP points at.
    ///
    /// ```text
    ///  -4   saved ebp  <- EBP after `mov ebp, esp`; `leave` pops it: frame[0xC]
    ///  -8   local      <- [ebp-4]: frame[0x8]
    /// ```
    #[test]
    fn plan_should_map_ebp_frame_and_leave() {
        // Arrange
        let model = analyzed_function(&[
            0x55, //                   1000: push ebp
            0x89, 0xE5, //             1001: mov  ebp, esp
            0x83, 0xEC, 0x08, //       1003: sub  esp, 8
            0x89, 0x45, 0xFC, //       1006: mov  [ebp-4], eax
            0x8B, 0x4D, 0xFC, //       1009: mov  ecx, [ebp-4]
            0xC9, //                   100C: leave
            0xC3, //                   100D: ret
        ]);

        // Act
        let locals = make_plan_for_model(&model).unwrap();

        // Assert
        assert_eq!(locals.size, 0x10);
        assert_eq!(
            frame_offsets(&locals),
            [
                (0x1000, vec![], Some(0xC)),
                (0x1006, vec![0x8], None),
                (0x1009, vec![0x8], None),
                (0x100C, vec![], Some(0xC)),
            ]
        );
    }

    /// `push [esp]` reads before ESP moves.
    #[test]
    fn plan_should_map_push_of_stack_slot() {
        // Arrange
        let model = analyzed_function(&[
            0x56, //                   1000: push esi
            0xFF, 0x34, 0x24, //       1001: push [esp]      ; reads saved esi
            0x58, //                   1004: pop  eax
            0x5E, //                   1005: pop  esi
            0xC3, //                   1006: ret
        ]);

        // Act
        let locals = make_plan_for_model(&model).unwrap();

        // Assert
        assert_eq!(
            frame_offsets(&locals),
            [
                (0x1000, vec![], Some(0xC)),
                (0x1001, vec![0xC], Some(0x8)),
                (0x1004, vec![], Some(0x8)),
                (0x1005, vec![], Some(0xC)),
            ]
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    /// None of these qualify.
    #[rstest]
    #[case::calls(&[
        0x56,                         // push esi
        0xE8, 0x00, 0x00, 0x00, 0x00, // call next      ; <- not a leaf
        0x5E,                         // pop  esi
        0xC3,                         // ret
    ])]
    #[case::address_escapes(&[
        0x56,                         // push esi
        0x8D, 0x04, 0x24,             // lea  eax, [esp] ; <- slot address leaks
        0x5E,                         // pop  esi
        0xC3,                         // ret
    ])]
    #[case::esp_copied(&[
        0x56,                         // push esi
        0x89, 0xE0,                   // mov  eax, esp   ; <- slot address leaks
        0x5E,                         // pop  esi
        0xC3,                         // ret
    ])]
    #[case::indexed(&[
        0x56,                         // push esi
        0x8B, 0x04, 0x8C,             // mov  eax, [esp+ecx*4] ; <- any slot
        0x5E,                         // pop  esi
        0xC3,                         // ret
    ])]
    #[case::own_helper(&[
        0x56,                         // push esi
        0x87, 0x04, 0x24,             // xchg [esp], eax ; <- atomic helper
        0x5E,                         // pop  esi
        0xC3,                         // ret
    ])]
    #[case::jump_with_slots_live(&[
        0x56,                         // push esi
        0xE9, 0x3A, 0x00, 0x00, 0x00, // jmp  0x1040 ; <- ESP not back at entry
    ])]
    #[case::no_slots(&[
        0x8B, 0x44, 0x24, 0x04,       // mov  eax, [esp+4] ; argument only
        0xC3,                         // ret
    ])]
    fn plan_should_refuse_function(#[case] code: &[u8]) {
        // Arrange
        let model = analyzed_function(code);

        // Act
        let locals = make_plan_for_model(&model);

        // Assert
        assert_eq!(locals, None);
    }
}
