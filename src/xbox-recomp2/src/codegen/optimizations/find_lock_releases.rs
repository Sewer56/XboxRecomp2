//! Finds `xchg`s that store zero, so we can write them as regular stores.

use super::helpers::instruction::register_access::RegisterAccess;
use super::helpers::instruction::stack_slot::is_stack_slot;
use crate::codegen::function::model::{FunctionModel, GuestOp, Hook, Op};
use crate::decode::{MachineMode, Mnemonic, Register, immediate, memory, register};

/// Marks each `xchg [mem], reg` where `reg` is zero as a lock release.
///
/// ```text
/// mov  eax, 1
/// xchg [edi], eax ; takes the lock, stays locked
/// ...
/// xor  edx, edx
/// xchg [edi], edx ; marked
/// ```
pub(super) fn run(model: &mut FunctionModel) {
    let ebp_frame = model.ebp_frame;
    let mut releases = 0;

    for block in &mut model.blocks {
        for index in 0..block.ops.len() {
            let reg = match &block.ops[index] {
                Op::Guest(guest) if !is_hooked(guest, &model.hooks) => {
                    match lock_register(guest, ebp_frame) {
                        Some(reg) => reg,
                        None => continue,
                    }
                }
                _ => continue,
            };

            // It's a release if it stores zero.
            if !is_zero_before(&block.ops[..index], reg, &model.hooks) {
                continue;
            }

            if let Op::Guest(guest) = &mut block.ops[index] {
                guest.releases_lock = true;
                releases += 1;
            }
        }
    }

    // No `xchg` left to take the lock?
    // Maybe not releases.
    if releases != 0 && !takes_lock(model) {
        unmark(model);
    }
}

/// Whether `model` still has an unmarked `xchg` on memory outside the stack.
/// That's the one that takes the lock.
fn takes_lock(model: &FunctionModel) -> bool {
    let ops = model.blocks.iter().flat_map(|block| &block.ops);
    for guest in ops.flat_map(Op::guests) {
        if !guest.releases_lock && lock_register(guest, model.ebp_frame).is_some() {
            return true;
        }
    }

    false
}

/// Clears every lock release mark in `model`.
fn unmark(model: &mut FunctionModel) {
    let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);
    for guest in ops.flat_map(Op::guests_mut) {
        guest.releases_lock = false;
    }
}

/// The register of an `xchg`.
fn lock_register(guest: &GuestOp, ebp_frame: bool) -> Option<Register> {
    if guest.insn.mnemonic() != Mnemonic::XCHG {
        return None;
    }

    let operands = &guest.insn.operands()[..2];
    let mem = operands.iter().find_map(memory)?;
    if is_stack_slot(mem, ebp_frame) {
        return None;
    }

    operands.iter().find_map(register)
}

/// Whether `reg` is zero by the end of `ops`.
/// The last write to `reg` must be `xor reg, reg` or `mov reg, 0`.
fn is_zero_before(ops: &[Op], reg: Register, hooks: &[Hook]) -> bool {
    for op in ops.iter().rev() {
        let guest = match op {
            Op::Guest(guest) if !is_hooked(guest, hooks) => guest,
            _ => return false,
        };

        if zeroes(guest, reg) {
            return true;
        }

        if RegisterAccess::of(&guest.insn).writes(reg) {
            return false;
        }
    }

    false
}

/// Whether `guest` zeroes all of `reg` with `xor r, r` or `mov r, 0`.
fn zeroes(guest: &GuestOp, reg: Register) -> bool {
    let operands = guest.insn.operands();
    let target = match operands.first().and_then(register) {
        Some(target) => target,
        None => return false,
    };

    if target != reg && target != reg.largest_enclosing(MachineMode::LEGACY_32) {
        return false;
    }

    let source = match operands.get(1) {
        Some(source) => source,
        None => return false,
    };

    match guest.insn.mnemonic() {
        Mnemonic::XOR => register(source) == Some(target),
        Mnemonic::MOV => immediate(source).is_some_and(|imm| imm.value == 0),
        _ => false,
    }
}

/// Whether a mid-assembly hook runs before `guest`.
fn is_hooked(guest: &GuestOp, hooks: &[Hook]) -> bool {
    hooks.iter().any(|hook| hook.address == guest.address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    #[rstest]
    #[case::xor_zero(
        &[
            0xB8, 0x01, 0x00, 0x00, 0x00, // 1000  mov  eax, 1
            0x87, 0x07,                   // 1005  xchg [edi], eax  ; takes the lock
            0x31, 0xD2,                   // 1007  xor  edx, edx
            0x87, 0x17,                   // 1009  xchg [edi], edx  ; marked
            0xC3,                         // 100B  ret
        ],
        &[0x1009],
    )]
    #[case::mov_zero_then_unrelated(
        &[
            0xB8, 0x01, 0x00, 0x00, 0x00, // 1000  mov  eax, 1
            0x87, 0x06,                   // 1005  xchg [esi], eax  ; takes the lock
            0xB8, 0x00, 0x00, 0x00, 0x00, // 1007  mov  eax, 0
            0x89, 0xF1,                   // 100C  mov  ecx, esi    ; EAX kept
            0x87, 0x06,                   // 100E  xchg [esi], eax  ; marked
            0xC3,                         // 1010  ret
        ],
        &[0x100E],
    )]
    #[case::no_lock_taken(
        &[
            0x31, 0xD2,                   // 1000  xor  edx, edx
            0x87, 0x17,                   // 1002  xchg [edi], edx  ; may take a value
            0xC3,                         // 1004  ret
        ],
        &[],
    )]
    #[case::register_changed(
        &[
            0xB8, 0x01, 0x00, 0x00, 0x00, // 1000  mov  eax, 1
            0x87, 0x07,                   // 1005  xchg [edi], eax  ; takes the lock
            0x31, 0xD2,                   // 1007  xor  edx, edx
            0x42,                         // 1009  inc  edx         ; EDX changed
            0x87, 0x17,                   // 100A  xchg [edi], edx
            0xC3,                         // 100C  ret
        ],
        &[],
    )]
    #[case::stack_slot(
        &[
            0x31, 0xD2,                   // 1000  xor  edx, edx
            0x87, 0x54, 0x24, 0x04,       // 1002  xchg [esp+4], edx ; this thread's stack
            0xC3,                         // 1006  ret
        ],
        &[],
    )]
    fn run_should_mark_xchg_only_when_register_is_zero(
        #[case] code: &[u8],
        #[case] expected: &[u32],
    ) {
        // Arrange
        let mut model = model_of(code);

        // Act
        run(&mut model);

        // Assert
        let marked: Vec<u32> = model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .flat_map(Op::guests)
            .filter(|guest| guest.releases_lock)
            .map(|guest| guest.address.get())
            .collect();
        assert_eq!(marked, expected);
    }
}
