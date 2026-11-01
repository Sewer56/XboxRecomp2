//! Find callback and pointer-table addresses in `push` and `mov`.

use super::{PointerScan, Resolution};
use crate::analysis::graph::{FunctionReference, Instruction, Operand, OperandKind, ReferenceKind};
use crate::decode::{DecodeOutcome, DecodedOperandKind, Mnemonic, Register};
use xbox_recomp2_executable::GuestAddress32;

impl PointerScan<'_> {
    /// Finds possible callbacks and function-pointer tables.
    ///
    /// Specifically we look for one of these
    ///
    /// ```asm
    /// push 0x1080              ; push a possible callback address
    /// mov eax, 0x1080          ; copy a possible callback address into eax
    /// mov dword [ecx], 0x2000  ; store a possible table address in memory
    /// ```
    ///
    /// We then check if the immediate value points to a recognisable function
    /// start or a function-pointer table.
    ///
    /// See function-discovery.md for more details.
    pub(super) fn scan_immediate(&mut self, instruction: &Instruction, result: &mut Resolution) {
        // Wrong instruction? You're out.
        let source = match instruction.mnemonic {
            Mnemonic::PUSH => instruction.operands.first(),
            Mnemonic::MOV => instruction.operands.get(1),
            _ => None,
        };
        let Some(value) = source.and_then(immediate_value).map(GuestAddress32::from) else {
            return;
        };

        if self.is_callback(value) {
            result.references.push(FunctionReference {
                site: instruction.address,
                target: value,
                kind: ReferenceKind::PossiblePointer,
            });
        } else {
            self.scan_table(instruction.address, value, None, result);
        }
    }

    /// Checks whether an immediate code address looks like a callback entry.
    ///
    /// Assuming the address points to the start of a function, firs
    /// instruction should be:
    /// - `push`, encoded in 1 to 5 bytes.
    /// - `mov`, encoded in 2 to 5 bytes.
    /// - `sub esp, N`, with a 32-bit destination
    /// - A direct `call` or `jmp` with a known target address.
    ///
    /// For more details, see `function-discovery.md`
    fn is_callback(&mut self, target: GuestAddress32) -> bool {
        // Rule out code we can't call
        if !self.is_callable(target) {
            return false;
        }

        // That ASCII check. If looks like ASCII, probably not callback,
        // but a sussy string.
        let code = self.code_window(target);
        if code[0] < 0x50
            || code
                .get(..8)
                .is_some_and(|bytes| bytes.iter().all(|byte| (0x20..=0x7e).contains(byte)))
        {
            return false;
        }

        // Can't decode? Not a callback.
        let DecodeOutcome::Ok(insn) = self.buffer.decode_at(self.decoder, code, 0) else {
            return false;
        };

        // Now check the instructions we can recognise
        match insn.mnemonic() {
            Mnemonic::PUSH => (1..=5).contains(&insn.length()),
            Mnemonic::MOV => (2..=5).contains(&insn.length()),
            Mnemonic::SUB => {
                // Functions without a frame pointer may reserve stack space first.
                let operands = insn.operands();
                matches!(operands[0].kind, DecodedOperandKind::Reg(Register::ESP))
                    && operands[0].size == 32
                    && matches!(&operands[1].kind, DecodedOperandKind::Imm(imm)
                        if !imm.is_relative && (1..=i32::MAX as u64).contains(&imm.value))
            }
            Mnemonic::CALL | Mnemonic::JMP => Instruction::from_decoded(target, insn)
                .direct_target()
                .is_some(),
            _ => false,
        }
    }
}

fn immediate_value(operand: &Operand) -> Option<u32> {
    if operand.width != 32 {
        return None;
    }
    match operand.kind {
        OperandKind::Immediate {
            value,
            relative: false,
        } => Some(value as u32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Check immediate callback recognition and unresolved computed addresses.

    use super::*;
    use crate::analysis::graph::test_helpers::base;
    use crate::analysis::pointer_tables::{
        test_helpers::resolution,
        test_helpers::{callback, executable},
    };
    use crate::config::RecompilerConfig;
    use rstest::rstest;

    // ── Core behaviour ───────────────────────────────────────────────────────

    #[rstest]
    #[case::small_frame(&[0x83, 0xec, 8])]
    #[case::large_frame(&[0x81, 0xec, 0, 1, 0, 0])]
    fn finds_callback_starting_with_sub_esp(#[case] prologue: &[u8]) {
        // Arrange
        //
        // `mov dword [ecx], 0x1080` receives a callback which
        // begins with `sub esp,N`, a typical stack frame setup instruction.
        //
        // ```text
        //   0x1000: C7 01 80 10 00 00  mov dword [ecx], 0x1080
        //   0x1006: C3                 ret
        // small_frame:
        //   0x1080: 83 EC 08           sub esp, 8
        // large_frame:
        //   0x1080: 81 EC 00 01 00 00  sub esp, 0x100
        // ```
        let code = [0xc7, 1, 0x80, 0x10, 0, 0, 0xc3];
        let mut executable = executable(&code, &[]);
        executable.data[0x80..0x80 + prologue.len()].copy_from_slice(prologue);

        // Act.
        let result = resolution(&code, &executable, &RecompilerConfig::default());

        // Assert.
        assert_eq!(
            result.references,
            [FunctionReference {
                site: base(),
                target: callback(),
                kind: ReferenceKind::PossiblePointer,
            }]
        );
    }

    #[rstest]
    #[case::push(&[0x68, 0x80, 0x10, 0, 0, 0xc3])]
    #[case::mov_register(&[0xb8, 0x80, 0x10, 0, 0, 0xc3])]
    #[case::mov_object(&[0xc7, 1, 0x80, 0x10, 0, 0, 0xc3])]
    fn finds_callback_in_push_or_mov(#[case] code: &[u8]) {
        // Arrange
        //
        // These instructions take the callback address without calling it.
        //
        // ```text
        // Case          0x1000 bytes       Instruction
        // push          68 80 10 00 00     push 0x1080
        // mov_register  B8 80 10 00 00     mov eax, 0x1080
        // mov_object    C7 01 80 10 00 00  mov dword [ecx], 0x1080
        //
        // Each instruction falls through to C3/ret at 0x1005 or 0x1006.
        // i.e. a case of a callback being returned.
        // ```
        let executable = executable(code, &[]);

        // Act.
        let result = resolution(code, &executable, &RecompilerConfig::default());

        // Assert.
        assert_eq!(
            result.references,
            [FunctionReference {
                site: base(),
                target: callback(),
                kind: ReferenceKind::PossiblePointer,
            }]
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    #[rstest]
    #[case::nonprologue(&[0xc3])]
    #[case::malformed(&[0xf0, 0x90])]
    #[case::ascii(b"PPPPPPPP")]
    #[case::breakpoint(&[0xcc])]
    #[case::other_register(&[0x83, 0xe8, 8])]
    #[case::partial_stack_pointer(&[0x66, 0x83, 0xec, 8])]
    #[case::memory_destination(&[0x83, 0x2c, 0x24, 8])]
    #[case::variable_stack_adjustment(&[0x29, 0xc4])]
    #[case::zero_stack_adjustment(&[0x83, 0xec, 0])]
    #[case::negative_byte_adjustment(&[0x83, 0xec, 0xf8])]
    #[case::negative_dword_adjustment(&[0x81, 0xec, 0xf8, 0xff, 0xff, 0xff])]
    fn ignores_targets_that_do_not_look_like_callbacks(#[case] target: &[u8]) {
        // Arrange.
        //
        // When callback function has unsupported instruction e.g.
        // int3, it probably isn't a valid callback. So skip it.
        //
        // ```text
        //   0x1000: 68 80 10 00 00  push 0x1080
        //   0x1005: C3              ret
        //   0x1080: C3              ret      ; nonprologue
        //        or F0 90           lock nop ; malformed
        //        or 50 x8           push eax ; bytes also spell ASCII "PPPPPPPP"
        //        or CC              int3     ; breakpoint
        // ```
        let code = [0x68, 0x80, 0x10, 0, 0, 0xc3];
        let mut executable = executable(&code, &[]);
        executable.data[0x80..0x80 + target.len()].copy_from_slice(target);

        // Act.
        let result = resolution(&code, &executable, &RecompilerConfig::default());

        // Assert.
        assert!(result.references.is_empty());
    }
}
