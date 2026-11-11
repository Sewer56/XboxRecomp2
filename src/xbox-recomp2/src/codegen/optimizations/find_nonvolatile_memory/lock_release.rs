//! Spots functions that release a lock with a normal `mov`.

use super::guests;
use crate::codegen::function::model::FunctionModel;
use crate::codegen::optimizations::helpers::instruction::stack_slot::is_stack_slot;
use crate::decode::{MemoryInfo, Mnemonic, memory};

/// Whether `model` takes a lock with `xchg` and releases it with a `mov`.
///
/// ```text
/// xchg [esi+4], eax     ; take the lock
/// mov  [esi+8], ecx
/// mov  dword [esi+4], 0 ; release it
/// ```
///
/// # Arguments
///
/// - `model`: the function.
/// - `locks`: a buffer for the lock addresses, reused between functions.
pub(super) fn releases_lock_with_mov(model: &FunctionModel, locks: &mut Vec<MemoryInfo>) -> bool {
    // Find the locks.
    locks.clear();
    for guest in guests(model) {
        if guest.insn.mnemonic() != Mnemonic::XCHG {
            continue;
        }

        for mem in guest.insn.operands()[..2].iter().filter_map(memory) {
            // Assume stack is thread specific, so isn't a lock.
            if !is_stack_slot(mem, model.ebp_frame) {
                locks.push(mem.clone());
            }
        }
    }

    if locks.is_empty() {
        return false;
    }

    // Find a `mov` that stores to one.
    for guest in guests(model) {
        if guest.insn.mnemonic() != Mnemonic::MOV {
            continue;
        }

        let target = memory(&guest.insn.operands()[0]);
        if target.is_some_and(|mem| locks.contains(mem)) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use rstest::rstest;

    #[rstest]
    #[case::released_with_mov(
        &[
            0x87, 0x46, 0x04,                         // xchg [esi+4], eax
            0xC7, 0x46, 0x04, 0x00, 0x00, 0x00, 0x00, // mov  dword [esi+4], 0 ; release
            0xC3,                                     // ret
        ],
        true,
    )]
    #[case::mov_to_other_address(
        &[
            0x87, 0x46, 0x04,                         // xchg [esi+4], eax
            0x89, 0x4E, 0x08,                         // mov  [esi+8], ecx     ; not the lock
            0xC3,                                     // ret
        ],
        false,
    )]
    #[case::stack_slot(
        &[
            0x87, 0x44, 0x24, 0x04,                   // xchg [esp+4], eax     ; this thread's stack
            0xC7, 0x44, 0x24, 0x04, 0x00, 0x00, 0x00, // mov  dword [esp+4], 0
            0x00,
            0xC3,                                     // ret
        ],
        false,
    )]
    fn releases_lock_with_mov_should_find_release(#[case] code: &[u8], #[case] expected: bool) {
        // Act
        let actual = releases_lock_with_mov(&model_of(code), &mut Vec::new());

        // Assert
        assert_eq!(actual, expected);
    }
}
