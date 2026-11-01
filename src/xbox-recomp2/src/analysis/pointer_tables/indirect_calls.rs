//! Find possible functions in calls through memory.

use super::{PointerScan, Resolution};
use crate::analysis::graph::{Flow, Instruction, MemoryOperand, OperandKind};
use crate::decode::Register;
use xbox_recomp2_executable::GuestAddress32;

impl PointerScan<'_> {
    /// Finds possible functions through indirect calls.
    ///
    /// We look for calls like these:
    ///
    /// ```asm
    /// call dword [0x2000]          ; one pointer at 0x2000
    /// call dword [eax*4 + 0x2000]  ; a table of pointers at 0x2000
    /// ```
    ///
    /// See `function-discovery.md` for more details.
    pub(super) fn scan_indirect_call(
        &mut self,
        instruction: &Instruction,
        result: &mut Resolution,
    ) {
        // Skip if not imm32
        let Some(operand) = instruction
            .operands
            .first()
            .filter(|operand| operand.width == 32)
        else {
            return;
        };

        // Process only `call dword [0x2000]`
        match operand.kind {
            OperandKind::Memory(memory) if instruction.flow == Flow::Call => {
                if let Some((start, count)) = call_table_range(memory) {
                    self.scan_table(
                        instruction.address,
                        GuestAddress32::from(start),
                        count,
                        result,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Gets where the pointers start and how many to read.
///
/// # Returns
///
/// - `Some((0x2000, Some(1)))` for `[0x2000]`: read one pointer.
/// - `Some((0x2000, None))` for `[eax*4 + 0x2000]`: read a table of any length.
/// - [`None`] for any other address, such as `fs:[0x2000]` or `[eax + 0x2000]`.
fn call_table_range(memory: MemoryOperand) -> Option<(u32, Option<u32>)> {
    if memory.address_width != 32 || memory.segment != Register::DS || memory.base != Register::NONE
    {
        return None;
    }
    let start = memory.displacement as u32;
    if memory.index == Register::NONE {
        // call dword [0x2000]
        return Some((start, Some(1)));
    }
    if memory.scale != 4 {
        return None;
    }

    // call dword [eax*4 + 0x2000]
    Some((start, None))
}

#[cfg(test)]
mod tests {
    //! Check which calls through memory we scan and which we skip.

    use crate::analysis::graph::{FunctionReference, ReferenceKind, test_helpers::at};
    use crate::analysis::pointer_tables::{
        test_helpers::resolution,
        test_helpers::{CALLBACK, UNALIGNED, callback, executable, unaligned},
    };
    use crate::config::RecompilerConfig;
    use rstest::rstest;
    use xbox_recomp2_executable::GuestAddress32;

    // ── Core behaviour ───────────────────────────────────────────────────────

    #[rstest]
    #[case::indexed_table(
        &[0xb8, 0, 0, 0, 0, 0xff, 0x14, 0x85, 0, 0x20, 0, 0, 0xc3],
        5,
        vec![callback(), unaligned()]
    )]
    #[case::single_pointer(&[0xff, 0x15, 0, 0x20, 0, 0, 0xc3], 0, vec![callback()])]
    fn finds_targets_in_memory_calls(
        #[case] code: &[u8],
        #[case] site: u32,
        #[case] expected: Vec<GuestAddress32>,
    ) {
        // Arrange
        //
        // With eax set to zero, the indexed call can only use the first entry.
        // The scan does not track eax, so it finds both functions in the table.
        //
        // ```text
        // Shared table:
        //   0x2000: dd 0x1080  ; entry 0
        //   0x2004: dd 0x1091  ; entry 1
        //
        // indexed_table:
        //   0x1000: mov eax, 0
        //   0x1005: call dword [eax*4 + 0x2000]
        //   0x100C: ret
        //
        // single_pointer:
        //   0x1000: call dword [0x2000]
        //   0x1006: ret
        // ```
        //
        // The call to [0x2000] reads just the first entry. Both cases produce
        // PossiblePointer because the guest could change the stored pointers.
        let executable = executable(code, &[CALLBACK, UNALIGNED]);

        // Act.
        let result = resolution(code, &executable, &RecompilerConfig::default());

        // Assert.
        assert_eq!(
            result.references,
            expected
                .into_iter()
                .map(|target| FunctionReference {
                    site: at(site),
                    target,
                    kind: ReferenceKind::PossiblePointer,
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn ignores_jumps() {
        // Arrange
        //
        // The pointer at 0x2000 points to a function, but this scan only checks
        // calls. A jump through the same pointer should find nothing.
        //
        // ```text
        //   0x2000: dd 0x1091
        //   0x1000: jmp dword [0x2000]   ; no reference
        // ```
        let code = &[0xff, 0x25, 0, 0x20, 0, 0];
        let executable = executable(code, &[UNALIGNED]);

        // Act.
        let result = resolution(code, &executable, &RecompilerConfig::default());

        // Assert.
        assert!(result.references.is_empty());
    }

    #[rstest]
    #[case::fs_segment(&[0x64, 0xff, 0x15, 0, 0x20, 0, 0, 0xc3])]
    #[case::base_register(&[0xff, 0x94, 0x88, 0, 0x20, 0, 0, 0xc3])]
    #[case::word_pointer(&[0x66, 0xff, 0x15, 0, 0x20, 0, 0, 0xc3])]
    #[case::address_16bit(&[0x67, 0xff, 0x16, 0, 0x20, 0xc3])]
    fn ignores_unsupported_addresses(#[case] code: &[u8]) {
        // Arrange
        //
        // None of these calls has an address this scan can use. They use FS,
        // a base register, a 16-bit pointer, or a 16-bit address.
        //
        // ```text
        // Case            0x1000 bytes             Instruction
        // fs_segment      64 FF 15 00 20 00 00     call dword fs:[0x2000]
        // base_register   FF 94 88 00 20 00 00     call dword [eax + ecx*4 + 0x2000]
        // word_pointer    66 FF 15 00 20 00 00     call word [0x2000]
        // address_16bit   67 FF 16 00 20           call dword [0x2000] ; 16-bit address
        // ```
        //
        // Each instruction is followed by C3/ret;  0x2000 holds dd 0x1091.
        let executable = executable(code, &[UNALIGNED]);

        // Act.
        let result = resolution(code, &executable, &RecompilerConfig::default());

        // Assert.
        assert!(result.references.is_empty());
    }
}
