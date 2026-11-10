//! Tracks where ESP and EBP point before each instruction in a function.
//!
//! stack-tracking.md in docs may be useful. my amigo

use super::known_callers::direct_call_target;
use super::op_graph::{OpGraph, OpIndex};
use super::stack_frame::StackFrame;
use crate::codegen::function::model::{FunctionModel, GuestOp, Op, Return};
use crate::config::FunctionAddress;
use crate::decode::{BranchType, InstructionCategory, Mnemonic};
use optional::Optioned;

/// What calling a function does to ESP and EBP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) struct CallEffect {
    /// Bytes its `ret` pops.
    /// Including the return address if there is one.
    ///
    /// i.e. `ret 0` -> 4
    ///      `ret 4` -> 8
    /// etc.
    pub popped: u32,

    /// Whether EBP is the same after the call.
    pub keeps_ebp: bool,
}

/// Where ESP and EBP point before each instruction.
pub(in crate::codegen) struct FunctionStack<'a> {
    /// The instructions, in the order they're written.
    pub instructions: Vec<StackInstruction<'a>>,

    /// Index of the first instruction to run.
    ///
    /// i.e. When the function entry point isn't the first instruction in the
    ///      list (not first address)
    pub entry: usize,

    /// 4 if callers push a return address, otherwise 0.
    pub return_slot_bytes: u32,

    graph: OpGraph<'a>,
}

/// One instruction, and where ESP and EBP point before it runs.
pub(in crate::codegen) struct StackInstruction<'a> {
    pub instruction: &'a GuestOp,

    /// Where ESP and EBP point, as offsets from the starting ESP.
    /// [`None`] if no path reaches the instruction.
    ///
    /// To find the stack slot the instruction uses, call [`StackFrame::address`].
    pub frame: Option<StackFrame>,

    // ── Instruction Specific Info ────────────────────────────────────────────
    /// The `ret` instruction.
    ///
    /// As we track the stack, we need to know how many bytes are popped off
    /// the stack. And with `ret`, that can vary. Hence we store it here as a
    /// bit of a 'hack'.
    pub return_op: Option<&'a Return>,

    /// For a call, how far it moves ESP down before entering the function.
    ///
    /// See [`Call::stack_bytes`].
    ///
    /// [`Call::stack_bytes`]: crate::codegen::function::model::Call::stack_bytes
    pub call_bytes: Option<u32>,
}

impl<'a> FunctionStack<'a> {
    /// Walks every path from the entry, tracking ESP and EBP.
    ///
    /// # Returns
    ///
    /// [`None`] if we can't follow the function:
    ///
    /// - It has hooks or decode failures.
    /// - It reaches a call that `calls` doesn't know, like `call eax`.
    /// - It reaches an indirect jump, or a jump [`is_branch`] rejects, like `loop`.
    ///   Switch tables are fine if every case stays in the function.
    /// - A path leaves the function without a `ret`, like a tail-call `jmp`.
    /// - It uses 16-bit addressing.
    /// - Two paths meet with different ESP or EBP.
    /// - A `ret` runs with ESP not back where it started.
    /// - A call moves ESP outside the `i32` range.
    /// - [`StackFrame::step`] fails.
    pub(in crate::codegen) fn new(
        model: &'a FunctionModel,
        calls: impl Fn(FunctionAddress) -> Option<CallEffect>,
    ) -> Option<Self> {
        // Bail out if we have hooks.
        if !model.hooks.is_empty() {
            return None;
        }

        // Collect the instructions.
        let instructions: Vec<_> = model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .map(|op| match op {
                Op::Guest(instruction) => Some(StackInstruction {
                    instruction,
                    frame: None,
                    return_op: None,
                    call_bytes: None,
                }),
                Op::CdqIdiv(idiv) => Some(StackInstruction {
                    instruction: &idiv.guest,
                    frame: None,
                    return_op: None,
                    call_bytes: None,
                }),
                // Collect the intrinsics too!
                Op::Call(call) => Some(StackInstruction {
                    instruction: &call.guest,
                    frame: None,
                    return_op: None,
                    call_bytes: Some(call.stack_bytes),
                }),
                Op::Return(return_op) => Some(StackInstruction {
                    instruction: &return_op.guest,
                    frame: None,
                    return_op: Some(return_op),
                    call_bytes: None,
                }),
                _ => None,
            })
            .collect::<Option<_>>()?;

        // Do callers still push a return address?
        //
        // One of our optimisations, `eliminate_return_slot`, can remove this,
        // so we need to check for future calculations.
        //
        // This is always set on either all or none items, so first returned is
        // a-ok. oki-doki-doki.
        let omit_return_slot = instructions
            .iter()
            .find_map(|item| item.return_op)
            .is_some_and(|ret| ret.omit_return_slot);

        // Work out the paths from the current blocks.
        let graph = OpGraph::new(model)?;
        let entry_ins_idx = graph.entry as usize;
        let mut result = Self {
            instructions,
            entry: entry_ins_idx,
            return_slot_bytes: if omit_return_slot { 0 } else { 4 },
            graph,
        };

        // Walk every path, stopping at instructions we've already seen.
        // (In case of loops or multiple paths converging.)
        let mut pending = vec![(entry_ins_idx, StackFrame::default())]; // queue, add initial ins
        while let Some((index, mut frame)) = pending.pop() {
            // Do we have an item for this instruction already?
            let item = &mut result.instructions[index];
            if let Some(previous) = item.frame {
                if previous != frame {
                    // We've seen this previously with different frame
                    // Analysis error!!
                    // Bail out.
                    return None;
                }

                // Aight, we seen it, skip.
                continue;
            }
            item.frame = Some(frame);
            let guest = item.instruction;
            let insn = &guest.insn;

            // Note: We bail out here with ? in case flow successors are not available.
            //       That's for out-of-function jumps or cases we can't analyze.
            let returns = result.graph.successors(index)?.is_empty();
            if insn.instruction().address_width != 32 {
                return None; // skip 16-bit insns
            }

            // Check the instruction has exactly the paths we expect.
            match insn.instruction().meta.category {
                InstructionCategory::RET => {
                    if insn.mnemonic() != Mnemonic::RET
                        || insn.instruction().operand_width != 32
                        || insn.instruction().meta.branch_type != BranchType::NEAR
                        || frame.esp != 0
                    {
                        // ret with unexpected esp
                        // likely analysis error.
                        return None;
                    }
                    continue;
                }
                InstructionCategory::CALL => {
                    // Is a call.
                    let effect = calls(direct_call_target(guest)?)?;
                    frame.esp = frame
                        .esp
                        .checked_sub_unsigned(item.call_bytes?)?
                        .checked_add_unsigned(effect.popped)?;

                    if !effect.keeps_ebp {
                        frame.ebp = Optioned::none();
                    }

                    // Add isns after the call to pending.
                    // (We skip frame.step)
                    pending.extend(result.successors(index).map(|next| (next, frame)));
                    continue;
                }
                InstructionCategory::COND_BR | InstructionCategory::UNCOND_BR => {
                    // We don't support some branch isns. So if unsupported, bail.
                    if !is_branch(insn.mnemonic()) {
                        return None;
                    }
                }
                _ if returns => {
                    return None;
                }
                _ => {}
            }

            // Step the frame, then pass it to every next instruction.
            frame.step(insn)?;
            pending.extend(result.successors(index).map(|next| (next, frame)));
        }
        Some(result)
    }

    /// Returns the indices of the instructions that can run after `index`.
    pub(in crate::codegen) fn successors(
        &self,
        index: OpIndex,
    ) -> impl Iterator<Item = OpIndex> + '_ {
        let targets = self.graph.successors(index).unwrap_or_default();
        targets.iter().map(|&target| target as OpIndex)
    }

    /// Returns the instructions that some path from the entry reaches.
    pub(in crate::codegen) fn reachable(&self) -> impl Iterator<Item = &StackInstruction<'a>> {
        self.instructions.iter().filter(|item| item.frame.is_some())
    }
}

/// Whether `mnemonic` is a jump the stack argument pass supports.
///
/// We leave out `loop`, because the recompiler can't emit it yet.
pub(in crate::codegen) fn is_branch(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::JMP
            | Mnemonic::JB
            | Mnemonic::JBE
            | Mnemonic::JECXZ
            | Mnemonic::JL
            | Mnemonic::JLE
            | Mnemonic::JNB
            | Mnemonic::JNBE
            | Mnemonic::JNL
            | Mnemonic::JNLE
            | Mnemonic::JNO
            | Mnemonic::JNP
            | Mnemonic::JNS
            | Mnemonic::JNZ
            | Mnemonic::JO
            | Mnemonic::JP
            | Mnemonic::JS
            | Mnemonic::JZ
    )
}

/// A `calls` lookup for [`FunctionStack::new`] that knows no functions.
pub(in crate::codegen) fn no_calls(_: FunctionAddress) -> Option<CallEffect> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::analyzed_function;
    use crate::decode::memory;
    use rstest::rstest;

    /// The function every test calls.
    const CALLEE: FunctionAddress = FunctionAddress(0x1040);

    /// Knows only [`CALLEE`], which ends with `ret 4`.
    /// It pops the return address and 1 argument.
    fn callee_only(target: FunctionAddress) -> Option<CallEffect> {
        (target == CALLEE).then_some(CallEffect {
            popped: 8,
            keeps_ebp: true,
        })
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    /// A typical function: frame setup, a loop, a call and two reads of a local.
    ///
    /// ```text
    ///  +0   return address
    ///  -4   saved EBP   <- EBP after `mov ebp, esp`
    ///  -8   local       <- read through [esp+4] and [ebp-4]
    /// -12   local       <- ESP after `sub esp, 8`
    /// -16   argument    <- pushed for CALLEE, which pops it
    /// ```
    ///
    /// Two paths reach `inc ecx`: one from above and one from `jnz`.
    /// Both paths must agree on ESP and EBP.
    #[test]
    fn new_should_track_frames_through_a_whole_function() {
        // Arrange
        // One row per instruction, with ESP and EBP before it runs.
        #[rustfmt::skip]
        let listing: [(&[u8], i32, Option<i32>); 13] = [
            // bytes                          ESP   EBP           address  instruction
            (&[0x55],                           0,  None),     // 1000     push ebp
            (&[0x89, 0xE5],                    -4,  None),     // 1001     mov ebp, esp
            (&[0x83, 0xEC, 0x08],              -4,  Some(-4)), // 1003     sub esp, 8
            (&[0x31, 0xC9],                   -12,  Some(-4)), // 1006     xor ecx, ecx
            (&[0x41],                         -12,  Some(-4)), // 1008     inc ecx
            (&[0x83, 0xF9, 0x03],             -12,  Some(-4)), // 1009     cmp ecx, 3
            (&[0x75, 0xFA],                   -12,  Some(-4)), // 100C     jnz top
            (&[0x6A, 0x01],                   -12,  Some(-4)), // 100E     push 1
            (&[0xE8, 0x2B, 0x00, 0x00, 0x00], -16,  Some(-4)), // 1010     call CALLEE
            (&[0x8B, 0x44, 0x24, 0x04],       -12,  Some(-4)), // 1015     mov eax, [esp+4]
            (&[0x8B, 0x45, 0xFC],             -12,  Some(-4)), // 1019     mov eax, [ebp-4]
            (&[0xC9],                         -12,  Some(-4)), // 101C     leave
            (&[0xC3],                           0,  None),     // 101D     ret
        ];
        let code: Vec<u8> = listing
            .iter()
            .flat_map(|row| row.0.iter().copied())
            .collect();
        let model = analyzed_function(&code);

        // Act
        let stack = FunctionStack::new(&model, callee_only).unwrap();

        // Assert
        let frames: Vec<_> = stack.instructions.iter().map(|item| item.frame).collect();
        let expected: Vec<_> = listing
            .iter()
            .map(|&(_, esp, ebp)| {
                Some(StackFrame {
                    esp,
                    ebp: ebp.into(),
                })
            })
            .collect();
        assert_eq!(frames, expected);

        // Both loads read the local at -8.
        for load in &stack.instructions[9..=10] {
            let address = memory(&load.instruction.insn.operands()[1]).unwrap();
            assert_eq!(load.frame.unwrap().address(address), Some(-8));
        }
    }

    /// Calls that differ from the usual `call` + `ret 4`.
    ///
    /// ```text
    /// 1000  push ebp
    /// 1001  mov ebp, esp   ESP -4
    /// 1003  call CALLEE    ESP -4, EBP -4
    /// 1008  pop ebp        <- checked here
    /// 1009  ret
    /// ```
    #[rstest]
    #[case::callee_changes_ebp(4, CallEffect { popped: 4, keeps_ebp: false }, None)]
    #[case::no_return_address(0, CallEffect { popped: 0, keeps_ebp: true }, Some(-4))]
    fn new_should_apply_call_effects(
        #[case] call_bytes: u32,
        #[case] effect: CallEffect,
        #[case] ebp: Option<i32>,
    ) {
        // Arrange
        let mut model = analyzed_function(&[0x55, 0x89, 0xE5, 0xE8, 0x38, 0, 0, 0, 0x5D, 0xC3]);
        for op in model.blocks.iter_mut().flat_map(|block| &mut block.ops) {
            if let Op::Call(call) = op {
                call.stack_bytes = call_bytes;
            }
        }

        // Act
        let stack = FunctionStack::new(&model, |target| {
            assert_eq!(target, CALLEE);
            Some(effect)
        })
        .unwrap();

        // Assert
        let pop = &stack.instructions[3];
        assert_eq!(
            pop.frame,
            Some(StackFrame {
                esp: -4,
                ebp: ebp.into()
            })
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    /// Each case is a function `new` can't follow.
    #[rstest]
    #[case::unknown_call(&[0xE8, 0x4B, 0, 0, 0, 0xC3])] // call 0x1050; ret
    #[case::indirect_call(&[0xFF, 0xD0, 0xC3])] // call eax; ret
    #[case::paths_disagree(&[0x50, 0x75, 0xFD, 0x58, 0xC3])] // top: push eax; jnz top; pop eax; ret
    #[case::unbalanced_ret(&[0x50, 0xC3])] // push eax; ret
    #[case::loop_instruction(&[0xE2, 0xFE, 0xC3])] // top: loop top; ret
    #[case::short_address(&[0x67, 0xE3, 0, 0xC3])] // jcxz next; ret
    #[case::indirect_jump(&[0xFF, 0xE0])] // jmp eax
    #[case::jump_out(&[0x74, 0x40, 0xC3])] // jz 0x1042; ret
    fn new_should_fail_on_functions_it_cannot_follow(#[case] code: &[u8]) {
        // Arrange
        let model = analyzed_function(code);

        // Act
        let stack = FunctionStack::new(&model, callee_only);

        // Assert
        assert!(stack.is_none());
    }
}
