//! Moves every stack access above a boundary by a fixed number of bytes.
//!
//! See `stack-shift.md` for more info.
//!
//! - [`StackShift::find`]: works out the new offsets, or gives up.
//! - [`StackShift::apply`]: writes them into the model.

use super::function_stack::FunctionStack;
use super::instruction::register_access::RegisterAccess;
use super::stack_frame::{FrameStep, STACK_WORD_BYTES, StackFrame};
use crate::codegen::function::model::{FunctionModel, Op};
use crate::decode::{Insn, Mnemonic, Register, memory, register};
use xbox_recomp2_executable::GuestAddress32;

/// A planned shift, list of addresses, operands and new displacements.
pub(in crate::codegen) struct StackShift {
    /// Sorted by address, so [`StackShift::apply`] can binary search.
    edits: Vec<Edit>,
}

/// One memory operand to change.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edit {
    /// The instruction's guest address.
    address: GuestAddress32,

    /// Index of the memory operand.
    operand: usize,

    /// The new displacement, like `8` in `[esp+8]`.
    displacement: i32,
}

impl StackShift {
    /// Makes plan for moving every stack access at or above
    /// `from` by `delta` bytes.
    ///
    /// # Arguments
    ///
    /// - `stack`: the function's stack positions, from [`FunctionStack::new`].
    /// - `from`: the boundary, in bytes from entry ESP. Argument 0 starts at 4.
    /// - `delta`: how far to move. Positive moves up.
    ///   When moving down, only `ret` may use the bytes we move into.
    ///
    /// # Returns
    ///
    /// The plan, or [`None`] if we can't be sure the shift is safe:
    ///
    /// For the remarks and other important info, see `stack-shift.md`.
    pub(in crate::codegen) fn find(
        stack: &FunctionStack<'_>,
        from: i32,
        delta: i32,
    ) -> Option<Self> {
        let fixed_end = fixed_end(from, delta)?;
        let mut edits = Vec::new();
        for (index, item) in stack.instructions.iter().enumerate() {
            // Skip unreachable instructions.
            let Some(frame) = item.frame else {
                continue;
            };
            let insn = &item.instruction.insn;

            // Get Operands (addresses) the instruction names, like `[esp+8]` or `[ebp+8]`.
            let visible = usize::from(insn.instruction().operand_count_visible);
            for (operand, value) in insn.operands().iter().take(visible).enumerate() {
                // A memory operand: `segment:[base + index*scale + displacement]`.
                //
                //   [esp + ecx*4 + 4]
                //    │     │       └─ displacement
                //    │     └─────── index
                //    └──────────── base
                let Some(mem) = memory(value) else { continue };

                // We can't tell where `[eax+ebp*4]` lands, so give up.
                if points_into_stack(mem.index, frame) {
                    return None;
                }

                // Other bases, like `[eax+4]`, stay as they are.
                if !points_into_stack(mem.base, frame) {
                    continue;
                }

                // `lea` doesn't touch memory itself, so we check its uses instead.
                let offset = frame.address(mem)?;
                let width = if insn.mnemonic() == Mnemonic::LEA {
                    check_pointer_uses(stack, index, offset, from, delta)?;
                    0
                } else {
                    access_width(insn, operand)?
                };

                if offset >= from {
                    // Shift to new displaceement
                    let shifted = mem.disp.displacement.checked_add(i64::from(delta))?;
                    offset.checked_add(delta)?;
                    edits.push(Edit {
                        address: item.instruction.address,
                        operand,
                        displacement: i32::try_from(shifted).ok()?,
                    });
                } else if offset.checked_add(width)? > fixed_end {
                    return None; // reaches into what moves, or where it moves to
                }
            }

            // Memory the instruction touches without naming it, like `push` and `ret`.
            if !implicit_access_is_safe(insn, frame, from, fixed_end, stack.return_slot_bytes) {
                return None;
            }
        }

        edits.sort_unstable();
        Some(Self { edits })
    }

    /// Writes the new displacements into `model`.
    ///
    /// # Arguments
    ///
    /// - `model`: the model [`Self::find`]'s `stack` came from.
    ///
    /// # Remarks
    ///
    /// Don't change `model` between [`Self::find`] and this call.
    pub(in crate::codegen) fn apply(&self, model: &mut FunctionModel) {
        for op in model.blocks.iter_mut().flat_map(|block| &mut block.ops) {
            let Op::Guest(guest) = op else { continue };

            // Find this instruction's edits, if any.
            let start = self
                .edits
                .partition_point(|edit| edit.address < guest.address);
            let edits = self.edits[start..]
                .iter()
                .take_while(|edit| edit.address == guest.address);

            for edit in edits {
                let mem = guest
                    .insn
                    .memory_mut(edit.operand)
                    .expect("edits name memory operands");
                mem.disp.displacement = i64::from(edit.displacement);
                mem.disp.has_displacement = true;
            }
        }
    }
}

/// Follows the pointer a `lea` makes.
/// See stack-shift.md for an illustrated example.
///
/// # Arguments
///
/// - `lea`: index of the `lea` in `stack.instructions`.
/// - `origin`: where the pointer points, in bytes from entry ESP.
///
/// # Returns
///
/// [`None`] if:
///
/// - The `lea` writes something other than a plain 32-bit register.
/// - Something copies or changes the pointer.
/// - A path returns while still holding it.
/// - [`check_dereference`] fails for any use.
fn check_pointer_uses(
    stack: &FunctionStack<'_>,
    lea: usize,
    origin: i32,
    from: i32,
    delta: i32,
) -> Option<()> {
    // The pointer must be in a general 32-bit register:
    // - ESP and EBP are tracked separately, not as movable argument pointers.
    // - `lea ax, [esp+4]` cuts off the address.
    let pointer = register(&stack.instructions[lea].instruction.insn.operands()[0])?;
    if !matches!(
        pointer,
        Register::EAX
            | Register::ECX
            | Register::EDX
            | Register::EBX
            | Register::ESI
            | Register::EDI
    ) {
        return None;
    }

    // Walk every path from the `lea`.
    // Loops can lead back to an instruction we've seen, so check each one only once.
    //
    // Sewer note to self- this makes a few places we walk all instructions, with
    // loops et al. Should probably dedupe at sooooome point.
    let mut pending: Vec<_> = stack.successors(lea).collect();
    let mut visited = vec![false; stack.instructions.len()];
    while let Some(index) = pending.pop() {
        if visited[index] {
            continue;
        }
        visited[index] = true;

        // Reading the pointer as a value copies or changes it.
        // e.g. `add eax, 4`.
        let insn = &stack.instructions[index].instruction.insn;
        let access = RegisterAccess::of(insn);
        if access.reads_value(pointer) {
            return None;
        }

        // Check every memory use, including hidden ones.
        let count = usize::from(insn.instruction().operand_count);
        for (operand, value) in insn.operands().iter().take(count).enumerate() {
            let Some(mem) = memory(value) else { continue };
            if mem.base == pointer || mem.index == pointer {
                check_dereference(insn, operand, pointer, origin, from, delta)?;
            }
        }

        // Only a whole-register `mov` ends the pointer, like `mov eax, 0`.
        if access.writes(pointer) {
            let destination = &insn.operands()[0];
            let overwrite = insn.mnemonic() == Mnemonic::MOV
                && register(destination) == Some(pointer)
                && destination.size == 32;
            if !overwrite {
                return None;
            }

            // This path is done.
            continue;
        }

        // No next instruction means we returned with the pointer still held.
        // The caller could use it, so give up.
        let mut successors = stack.successors(index).peekable();
        successors.peek()?;
        pending.extend(successors);
    }
    Some(())
}

/// Checks the stack accesses `insn` makes without naming an address.
fn implicit_access_is_safe(
    insn: &Insn,
    frame: StackFrame,
    from: i32,
    fixed_end: i32,
    return_slot_bytes: u32,
) -> bool {
    // `ret` pops the return address at ESP, if callers push one.
    if insn.mnemonic() == Mnemonic::RET {
        return frame
            .esp
            .checked_add_unsigned(return_slot_bytes)
            .is_some_and(|end| end <= from);
    }

    let word = STACK_WORD_BYTES as i32;
    let mut next = frame;
    match next.step(insn) {
        // These forms need their own checks before shifting arguments.
        Some(FrameStep::PushOther(_) | FrameStep::PopOther(_)) => false,
        // `push` writes the slot just below ESP.
        Some(FrameStep::Push(reg)) => !points_into_stack(reg, frame) && frame.esp <= fixed_end,
        Some(FrameStep::Adjust) if insn.mnemonic() == Mnemonic::PUSH => frame.esp <= fixed_end,

        // `pop` reads the slot at ESP. `leave` pops the one at EBP.
        Some(FrameStep::Pop(_)) => frame
            .esp
            .checked_add(word)
            .is_some_and(|end| end <= fixed_end),
        Some(FrameStep::Restore) if insn.mnemonic() == Mnemonic::LEAVE => frame
            .ebp
            .into_option()
            .and_then(|ebp| ebp.checked_add(word))
            .is_some_and(|end| end <= fixed_end),

        // Anything else must not copy ESP or EBP, like `mov eax, esp`.
        Some(FrameStep::Unchanged) => {
            let access = RegisterAccess::of(insn);
            ![Register::ESP, Register::EBP]
                .into_iter()
                .any(|reg| points_into_stack(reg, frame) && access.reads_value(reg))
        }

        // Steps like `sub esp, 8` or `mov ebp, esp` only change registers we track.
        Some(_) => true,
        None => false, // can't follow ESP
    }
}

/// Checks one use of a `lea` pointer, like `mov ecx, [eax+4]`.
///
/// The access must be a plain `[pointer+N]`.
/// It must also stay on the same side of `from` as `origin`.
fn check_dereference(
    insn: &Insn,
    operand: usize,
    pointer: Register,
    origin: i32,
    from: i32,
    delta: i32,
) -> Option<()> {
    // Another `lea`, like `lea ecx, [eax+4]`, copies a pointer.
    if insn.mnemonic() == Mnemonic::LEA {
        return None;
    }

    // An index like `[eax+edx*4]`. Hides offset.
    // A segment like `fs:` points somewhere other than the stack.
    let mem = memory(&insn.operands()[operand])?;
    let plain = mem.base == pointer
        && mem.index == Register::NONE
        && matches!(mem.segment, Register::DS | Register::SS);
    if !plain {
        return None;
    }

    // Where the access starts and ends, in bytes from entry ESP.
    let start = origin.checked_add(i32::try_from(mem.disp.displacement).ok()?)?;
    let end = start.checked_add(access_width(insn, operand)?)?;

    if origin >= from {
        // The pointer moves, so this access moves with it.
        // It must start at or above `from`, and its first and last byte
        // must still fit in an `i32` once moved.
        if start < from {
            return None;
        }
        start.checked_add(delta)?;
        (end - 1).checked_add(delta)?;
    } else if end > fixed_end(from, delta)? {
        return None; // stays put, but reaches into what moves, or where it moves to
    }
    Some(())
}

/// Whether `reg` is ESP, or EBP while it points into the stack.
fn points_into_stack(reg: Register, frame: StackFrame) -> bool {
    reg == Register::ESP || (frame.ebp.is_some() && reg == Register::EBP)
}

/// Returns how many bytes `insn` touches at memory `operand`.
///
/// Returns [`None`] if we can't tell from the operand's size:
///
/// - A hidden operand, like the one `xlat` reads at EBX.
/// - `bt`, `bts`, `btr` or `btc` with a register bit offset.
///   These can reach `offset / 8` bytes away, like `bt [esp], ecx`.
/// - An operand with no size.
fn access_width(insn: &Insn, operand: usize) -> Option<i32> {
    if operand >= usize::from(insn.instruction().operand_count_visible) {
        return None;
    }

    let bit_test = matches!(
        insn.mnemonic(),
        Mnemonic::BT | Mnemonic::BTS | Mnemonic::BTR | Mnemonic::BTC
    );
    if bit_test && register(&insn.operands()[1]).is_some() {
        return None;
    }

    let bytes = insn.operands()[operand].size.div_ceil(8);
    (bytes != 0).then_some(i32::from(bytes))
}

/// Where accesses that stay put must end, in bytes from entry ESP.
///
/// e.g. Moving `+4` down by 4 puts argument 0 at `+0`.
/// So nothing else can use `+0..+4`.
fn fixed_end(from: i32, delta: i32) -> Option<i32> {
    from.checked_add(delta.min(0))
}

#[cfg(test)]
mod tests {
    use super::super::function_stack::no_calls;
    use super::*;
    use crate::codegen::optimizations::helpers::tests::analyzed_function;
    use rstest::rstest;

    /// Shifts everything in `code` from argument 0 (entry ESP + 4) up by 4.
    fn shift(code: &[u8]) -> Option<Vec<i64>> {
        let mut model = analyzed_function(code);
        let stack = FunctionStack::new(&model, no_calls, false).unwrap();
        StackShift::find(&stack, 4, 4)?.apply(&mut model);
        Some(displacements(&model))
    }

    /// The displacement of each visible memory operand, in order.
    fn displacements(model: &FunctionModel) -> Vec<i64> {
        let mut result = Vec::new();
        for op in model.blocks.iter().flat_map(|block| &block.ops) {
            let Op::Guest(guest) = op else { continue };
            let visible = usize::from(guest.insn.instruction().operand_count_visible);
            for value in guest.insn.operands().iter().take(visible) {
                if let Some(mem) = memory(value) {
                    result.push(mem.disp.displacement);
                }
            }
        }
        result
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    #[rstest]
    #[case::argument(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0: moves
            0xC2, 4, 0,             // ret  4
        ],
        &[8],
    )]
    #[case::after_push(
        &[
            0x56,                   // push esi
            0x8B, 0x44, 0x24, 8,    // mov  eax, [esp+8]    ; argument 0: moves
            0x5E,                   // pop  esi
            0xC2, 4, 0,             // ret  4
        ],
        &[12],
    )]
    #[case::frame_pointer(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x8B, 0x45, 8,          // mov  eax, [ebp+8]    ; argument 0: moves
            0x5D,                   // pop  ebp
            0xC2, 4, 0,             // ret  4
        ],
        &[12],
    )]
    #[case::local(
        &[
            0x83, 0xEC, 8,          // sub  esp, 8
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; a local: stays put
            0x83, 0xC4, 8,          // add  esp, 8
            0xC3,                   // ret
        ],
        &[4],
    )]
    #[case::push_constant(
        &[
            0x6A, 1,                // push 1               ; writes below the boundary
            0x58,                   // pop  eax
            0xC3,                   // ret
        ],
        &[],
    )]
    fn find_should_move_accesses_above_boundary(#[case] code: &[u8], #[case] expected: &[i64]) {
        assert_eq!(shift(code).unwrap(), expected);
    }

    /// Any instruction works, as long as it touches only its operand's bytes.
    #[rstest]
    #[case::math(
        &[
            0x01, 0x44, 0x24, 4,        // add  [esp+4], eax
            0xC3,                       // ret
        ],
        &[8],
    )]
    #[case::compare(
        &[
            0x83, 0x7C, 0x24, 4, 0,     // cmp  dword [esp+4], 0
            0xC3,                       // ret
        ],
        &[8],
    )]
    #[case::byte_load(
        &[
            0x0F, 0xB6, 0x44, 0x24, 4,  // movzx eax, byte [esp+4]
            0xC3,                       // ret
        ],
        &[8],
    )]
    #[case::x87(
        &[
            0xD9, 0x44, 0x24, 4,        // fld  dword [esp+4]
            0xC3,                       // ret
        ],
        &[8],
    )]
    #[case::push_memory(
        &[
            0xFF, 0x74, 0x24, 4,        // push [esp+4]
            0x58,                       // pop  eax
            0xC3,                       // ret
        ],
        &[8],
    )]
    #[case::bit_test_constant(
        &[
            0x0F, 0xBA, 0x64, 0x24, 4, 3,   // bt   dword [esp+4], 3    ; stays in its 4 bytes
            0xC3,                           // ret
        ],
        &[8],
    )]
    fn find_should_move_any_sized_access(#[case] code: &[u8], #[case] expected: &[i64]) {
        assert_eq!(shift(code).unwrap(), expected);
    }

    /// The `lea` moves with its target. Uses through the pointer stay as is.
    #[rstest]
    #[case::pointer_above(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]    ; moves
            0x8B, 0x48, 4,          // mov  ecx, [eax+4]
            0xB8, 0, 0, 0, 0,       // mov  eax, 0          ; overwrites the pointer
            0xC3,                   // ret
        ],
        &[8, 4],
    )]
    #[case::pointer_below(
        &[
            0x8D, 0x04, 0x24,       // lea  eax, [esp]      ; stays put
            0x8B, 0x08,             // mov  ecx, [eax]
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
        &[0, 0],
    )]
    #[case::frame_pointer(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x8D, 0x45, 8,          // lea  eax, [ebp+8]    ; moves
            0x8B, 0x48, 4,          // mov  ecx, [eax+4]
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0x5D,                   // pop  ebp
            0xC3,                   // ret
        ],
        &[12, 4],
    )]
    #[case::overwritten_on_both_paths(
        &[
            0x8D, 0x44, 0x24, 4,    //     lea  eax, [esp+4]    ; moves
            0x85, 0xD2,             //     test edx, edx
            0x74, 2,                //     jz   .b
            0x8B, 0x08,             //     mov  ecx, [eax]
            0xB8, 0, 0, 0, 0,       // .b: mov  eax, 0
            0xC3,                   //     ret
        ],
        &[8, 0],
    )]
    #[case::math(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]    ; moves
            0x03, 0x08,             // add  ecx, [eax]
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
        &[8, 0],
    )]
    fn find_should_move_pointers_above_boundary(#[case] code: &[u8], #[case] expected: &[i64]) {
        assert_eq!(shift(code).unwrap(), expected);
    }

    /// Removing an op changes indices, but not addresses.
    #[test]
    fn apply_should_find_instructions_by_address() {
        // Arrange
        // Drop the `nop`, so the load moves from index 1 to 0.
        #[rustfmt::skip]
        let code = [
            0x90,                   // nop
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]
            0xC3,                   // ret
        ];
        let mut model = analyzed_function(&code);
        model.blocks[0].ops.remove(0);

        // Act
        // Shift by 4 twice, so the second `find` sees the first one's edits.
        for _ in 0..2 {
            let stack = FunctionStack::new(&model, no_calls, false).unwrap();
            StackShift::find(&stack, 4, 4).unwrap().apply(&mut model);
        }

        // Assert
        assert_eq!(displacements(&model), [12]); // <- 4 + 4 + 4
    }

    // ── Giving up ────────────────────────────────────────────────────────────

    #[rstest]
    #[case::stack_pointer_value(
        &[
            0x89, 0xE0,             // mov  eax, esp        ; copies ESP
            0xC3,                   // ret
        ],
    )]
    #[case::frame_pointer_value(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x55,                   // push ebp             ; copies EBP
            0x5D,                   // pop  ebp
            0x5D,                   // pop  ebp
            0xC3,                   // ret
        ],
    )]
    #[case::indexed(
        &[
            0x8B, 0x44, 0x8C, 4,    // mov  eax, [esp+ecx*4+4]  ; unknown offset
            0xC2, 4, 0,             // ret  4
        ],
    )]
    #[case::crossing(
        &[
            0x8B, 0x44, 0x24, 2,    // mov  eax, [esp+2]    ; bytes 2-5: crosses 4
            0xC2, 4, 0,             // ret  4
        ],
    )]
    #[case::wide_crossing(
        &[
            0xDD, 0x04, 0x24,       // fld  qword [esp]     ; bytes 0-7: crosses 4
            0xC3,                   // ret
        ],
    )]
    fn find_should_reject_unsafe_accesses(#[case] code: &[u8]) {
        assert_eq!(shift(code), None);
    }

    #[rstest]
    #[case::escapes(
        &[
            0x8D, 0x54, 0x24, 8,    // lea  edx, [esp+8]
            0xC2, 8, 0,             // ret  8               ; still in edx
        ],
    )]
    #[case::escapes_on_one_path(
        &[
            0x8D, 0x44, 0x24, 4,    //     lea  eax, [esp+4]
            0x85, 0xC9,             //     test ecx, ecx
            0x74, 5,                //     jz   .b
            0xB8, 0, 0, 0, 0,       //     mov  eax, 0
            0xC3,                   // .b: ret              ; still in eax via jz
        ],
    )]
    #[case::below_to_above(
        &[
            0x8D, 0x04, 0x24,       // lea  eax, [esp]      ; stays put
            0x8B, 0x48, 4,          // mov  ecx, [eax+4]    ; but reads argument 0
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::above_to_below(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]    ; moves
            0x8B, 0x48, 0xFC,       // mov  ecx, [eax-4]    ; but reads the return address
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::frame_below_to_above(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x8D, 0x45, 4,          // lea  eax, [ebp+4]    ; stays put
            0x8B, 0x48, 4,          // mov  ecx, [eax+4]    ; but reads argument 0
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0x5D,                   // pop  ebp
            0xC3,                   // ret
        ],
    )]
    #[case::frame_above_to_below(
        &[
            0x55,                   // push ebp
            0x89, 0xE5,             // mov  ebp, esp
            0x8D, 0x45, 8,          // lea  eax, [ebp+8]    ; moves
            0x8B, 0x48, 0xFC,       // mov  ecx, [eax-4]    ; but reads the return address
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0x5D,                   // pop  ebp
            0xC3,                   // ret
        ],
    )]
    #[case::indexed(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]
            0x8B, 0x0C, 0x90,       // mov  ecx, [eax+edx*4]    ; unknown offset
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::copied(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]
            0x89, 0xC2,             // mov  edx, eax        ; copy
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::changed(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]
            0x83, 0xC0, 4,          // add  eax, 4          ; arithmetic
            0x8B, 0x08,             // mov  ecx, [eax]
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::copied_by_lea(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]
            0x8D, 0x48, 4,          // lea  ecx, [eax+4]    ; copy
            0xB8, 0, 0, 0, 0,       // mov  eax, 0
            0xC3,                   // ret
        ],
    )]
    #[case::hidden_use(
        &[
            0x8D, 0x1C, 0x24,       // lea  ebx, [esp]
            0xD7,                   // xlat                 ; reads up to [ebx+255]
            0xBB, 0, 0, 0, 0,       // mov  ebx, 0
            0xC3,                   // ret
        ],
    )]
    #[case::partly_overwritten(
        &[
            0x8D, 0x44, 0x24, 4,    // lea  eax, [esp+4]
            0xB0, 0,                // mov  al, 0           ; rest of eax survives
            0xC3,                   // ret
        ],
    )]
    fn find_should_reject_unsafe_pointers(#[case] code: &[u8]) {
        assert_eq!(shift(code), None);
    }

    /// `bt` and friends can reach `ecx / 8` bytes outside their operand.
    #[rstest]
    #[case::direct_above(
        &[
            0xB9, 0xE0, 0xFF, 0xFF, 0xFF,   // mov  ecx, -32
            0x0F, 0xA3, 0x4C, 0x24, 4,      // bt   [esp+4], ecx    ; reads [esp]
            0xC3,                           // ret
        ],
    )]
    #[case::direct_below(
        &[
            0xB9, 32, 0, 0, 0,              // mov  ecx, 32
            0x0F, 0xA3, 0x0C, 0x24,         // bt   [esp], ecx      ; reads [esp+4]
            0xC3,                           // ret
        ],
    )]
    #[case::frame_above(
        &[
            0x55,                           // push ebp
            0x89, 0xE5,                     // mov  ebp, esp
            0xB9, 0xE0, 0xFF, 0xFF, 0xFF,   // mov  ecx, -32
            0x0F, 0xA3, 0x4D, 8,            // bt   [ebp+8], ecx    ; reads [ebp+4]
            0x5D,                           // pop  ebp
            0xC3,                           // ret
        ],
    )]
    #[case::frame_below(
        &[
            0x55,                           // push ebp
            0x89, 0xE5,                     // mov  ebp, esp
            0xB9, 32, 0, 0, 0,              // mov  ecx, 32
            0x0F, 0xA3, 0x4D, 4,            // bt   [ebp+4], ecx    ; reads [ebp+8]
            0x5D,                           // pop  ebp
            0xC3,                           // ret
        ],
    )]
    #[case::pointer_above(
        &[
            0xB9, 0xE0, 0xFF, 0xFF, 0xFF,   // mov  ecx, -32
            0x8D, 0x44, 0x24, 4,            // lea  eax, [esp+4]
            0x0F, 0xA3, 0x08,               // bt   [eax], ecx      ; reads [esp]
            0xB8, 0, 0, 0, 0,               // mov  eax, 0
            0xC3,                           // ret
        ],
    )]
    #[case::pointer_below(
        &[
            0xB9, 32, 0, 0, 0,              // mov  ecx, 32
            0x8D, 0x04, 0x24,               // lea  eax, [esp]
            0x0F, 0xA3, 0x08,               // bt   [eax], ecx      ; reads [esp+4]
            0xB8, 0, 0, 0, 0,               // mov  eax, 0
            0xC3,                           // ret
        ],
    )]
    #[case::bit_set(
        &[
            0x0F, 0xAB, 0x4C, 0x24, 4,      // bts  [esp+4], ecx
            0xC3,                           // ret
        ],
    )]
    #[case::bit_reset(
        &[
            0x0F, 0xB3, 0x0C, 0x24,         // btr  [esp], ecx
            0xC3,                           // ret
        ],
    )]
    #[case::bit_complement(
        &[
            0x0F, 0xBB, 0x4C, 0x24, 4,      // btc  [esp+4], ecx
            0xC3,                           // ret
        ],
    )]
    fn find_should_reject_bit_tests(#[case] code: &[u8]) {
        assert_eq!(shift(code), None);
    }

    // ── Moving down ──────────────────────────────────────────────────────────

    /// Shifts everything from argument 0 down by 4, onto the return address.
    fn shift_down(code: &[u8]) -> Option<Vec<i64>> {
        let mut model = analyzed_function(code);
        let stack = FunctionStack::new(&model, no_calls, false).unwrap();
        StackShift::find(&stack, 4, -4)?.apply(&mut model);
        Some(displacements(&model))
    }

    #[rstest]
    #[case::argument(
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; argument 0: moves to [esp]
            0xC2, 4, 0,             // ret  4               ; pops the return address
        ],
        &[0],
    )]
    #[case::local(
        &[
            0x83, 0xEC, 4,          // sub  esp, 4
            0x89, 0x04, 0x24,       // mov  [esp], eax      ; a local, below the return address
            0x83, 0xC4, 4,          // add  esp, 4
            0xC3,                   // ret
        ],
        &[0],
    )]
    fn find_should_move_down_onto_return_address(#[case] code: &[u8], #[case] expected: &[i64]) {
        assert_eq!(shift_down(code).unwrap(), expected);
    }
}
