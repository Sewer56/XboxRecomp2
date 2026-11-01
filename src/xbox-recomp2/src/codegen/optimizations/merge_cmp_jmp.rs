//! Merges a compare with the jumps that read its flags.
//!
//! Follows "Step by step" in `merge-compare-and-jump.md`:
//!
//! - [`plan`] does steps 1 to 3.
//! - `codegen/function/write/emit/instructions/merged_cmp_jmp/` does step 4.

use super::helpers::flag_boundaries::FlagBoundaries;
use super::helpers::instruction::flag_effects::EFlagEffects;
use super::helpers::needed_flags::NeededFlags;
use crate::codegen::function::model::{
    ALL_ARITHMETIC_FLAGS, BlockOpIndex, FunctionModel, GuestOp, Hook, MergedCmpJmp,
    MergedInstruction, MergedInstructionKind, Op,
};
use crate::decode::{CpuFlag, Insn, InstructionCategory, Mnemonic, Register, register};
use core::mem;

/// Most instructions we check after a compare.
const MAX_INSTRUCTIONS: usize = 16;

/// A compare and the instructions merging with it.
struct Plan {
    /// Where the compare is in its block.
    compare: BlockOpIndex,

    /// Whether an `fstp st0` right after an x87 compare merges too.
    ///
    /// See the x87 example in the markdown doc- mmkay?
    x87_pop: bool,

    /// How to write each instruction after the compare (and x87 pop).
    instructions: Vec<MergedInstructionKind>,

    /// Flags needed after the last instruction.
    end_flags: CpuFlag,
}

impl Plan {
    /// Where the last instruction is in its block.
    fn last(&self) -> BlockOpIndex {
        BlockOpIndex::new(self.compare.get() + usize::from(self.x87_pop) + self.instructions.len())
    }
}

/// Merges each compare with its eflags readers, when that skips writing flags.
pub(super) fn run(models: &mut [FunctionModel], boundaries: &FlagBoundaries) {
    for model in models {
        merge_function(model, boundaries);
    }
}

/// Merges each compare in one function with its eflags readers.
fn merge_function(model: &mut FunctionModel, boundaries: &FlagBoundaries) {
    // Find out which flags are needed at each instruction.
    let Some(live) = NeededFlags::new(model, boundaries) else {
        return;
    };

    let mut start: usize = 0; // <- where the block starts in `live`
    let mut plans = Vec::new();

    // Now go through all the blocks (straight lines of instructions).
    // We optimise here at block level so brr brr off you go.
    for block in &mut model.blocks {
        let mut index = 0;
        while index < block.ops.len() {
            // Try finding a compare at this index and form a plan.
            match plan(&block.ops, index, start, &live, &model.hooks) {
                Some(plan) => {
                    index = plan.last().get() + 1;
                    plans.push(plan);
                }
                None => index += 1,
            }
        }

        start += block.ops.len();

        // Replace the OG instructions in this block with
        // our merged branch. Old instructions yeeted out.
        // New instructions swapped in!
        if !plans.is_empty() {
            merge_into_ops(&mut block.ops, plans.drain(..));
        }
    }
}

/// Replaces each planned compare and its instructions with one [`MergedCmpJmp`].
///
/// # Examples
///
/// A block with two compares gets two plans:
///
/// ```text
/// index:   0    1    2   3    4   5
/// before: [mov, cmp, jz, cmp, jz, ret]
///
/// plan 1:  compare at 1, last at 2
/// plan 2:  compare at 3, last at 4
///
/// after:  [mov, MergedCmpJmp(cmp, jz), MergedCmpJmp(cmp, jz), ret]
/// ```
///
/// # Remarks
///
/// `plans` must be in block order and must not overlap.
///
/// We rebuild `ops` in one pass, so each plan's indices still match the
/// original list.
fn merge_into_ops(ops: &mut Vec<Op>, plans: impl Iterator<Item = Plan>) {
    let len = ops.len();
    let mut old_ops = mem::replace(ops, Vec::with_capacity(len)).into_iter();
    let mut taken = 0;

    for plan in plans {
        // Copy the ops before the compare
        ops.extend(old_ops.by_ref().take(plan.compare.get() - taken));
        taken = plan.last().get() + 1;

        // Now append the new operations (as intrinsics) to ops.
        let compare = next_guest(&mut old_ops);
        let x87_pop = plan.x87_pop.then(|| next_guest(&mut old_ops));
        let instructions = plan
            .instructions
            .into_iter()
            .map(|kind| MergedInstruction {
                guest: next_guest(&mut old_ops),
                kind,
            })
            .collect();

        ops.push(Op::MergedCmpJmp(Box::new(MergedCmpJmp {
            compare,
            x87_pop,
            instructions,
            end_flags: plan.end_flags,
        })));
    }

    // Copy the ops after the last plan.
    ops.extend(old_ops);
}

/// Takes the next op, which the plan says is a guest instruction.
fn next_guest(ops: &mut impl Iterator<Item = Op>) -> GuestOp {
    match ops.next() {
        Some(Op::Guest(guest)) => guest,
        _ => unreachable!("plans cover only guest ops"),
    }
}

/// Makes a plan to merge the compare at `index` with the instructions after it.
///
/// # Arguments
///
/// - `ops`: the block's ops.
/// - `index`: where the compare might be in `ops`.
/// - `start`: where the block starts in `live`.
/// - `live`: the flags needed around each op in the function.
/// - `hooks`: the function's midasm hooks.
///
/// # Returns
///
/// [`None`] when nothing merges, or merging would still need the flags written.
fn plan(
    ops: &[Op],
    index: usize,
    start: usize,
    live: &NeededFlags,
    hooks: &[Hook],
) -> Option<Plan> {
    // PS. PS. Sewer sez.
    //
    // Look at the example in `merge-compare-and-jump.md`.
    // I've laid out the code to roughly follow it, so you'll be able to follow
    // along too!

    let hooked = |guest: &GuestOp| hooks.iter().any(|hook| hook.address == guest.address);

    // Step 1: find a compare (at index)
    //
    // cmp cl, [edi]   ; <- compare
    let Op::Guest(compare) = &ops[index] else {
        return None; // not an instruction, bail
    };
    if hooked(compare) || !MergedCmpJmp::accepts(compare) {
        return None; // hooked, or a compare we can't merge, bail
    }
    let mnemonic = compare.insn.mnemonic();
    let is_x87_cmp = matches!(mnemonic, Mnemonic::FCOMIP | Mnemonic::FUCOMIP);
    let is_float = is_x87_cmp || matches!(mnemonic, Mnemonic::COMISS | Mnemonic::UCOMISS);

    // Float compares and `sahf` have no helper that writes only some flags.
    let can_write_flags = !is_float && mnemonic != Mnemonic::SAHF;

    // Step 2: look at what follows.
    //
    // cmp cl, [edi]
    // mov [esi], eax  ; no flags: goes along
    // jnz differ      ; reads flags: merged
    // cmp ecx, edx    ; writes flags: stop
    //
    // A block ends at a jump target, so we stop there too.
    let mut x87_pop = false;
    let mut instructions = Vec::new();
    let mut merged_len = 0; // <- instructions up to the last eflags reader
    for (op_index, op) in ops
        .iter()
        .enumerate()
        .skip(index + 1)
        .take(MAX_INSTRUCTIONS)
    {
        // Stop at anything that isn't a guest instruction, or is hooked.
        let Op::Guest(next) = op else { break };
        if hooked(next) {
            break;
        }

        // Special case: an x87 compare followed by `fstp st0`.
        //
        // fcomip st0, st1  ; compare, then pop st0
        // fstp   st0       ; pop again: both values are gone
        // jb     below     ; reads CF from fcomip
        //
        // We merge the `fstp` too.
        // See `merge-compare-and-jump.md` for the C++ we write.
        if op_index == index + 1 && is_x87_cmp && is_pop_st0(&next.insn) {
            x87_pop = true;
            continue;
        }

        // Check if the cmp (compare.insn) from earlier can be merged with the
        // (next.insn) [following instruction].
        let kind = match MergedCmpJmp::can_merge(&compare.insn, &next.insn) {
            Some(kind) => kind,
            None if is_plain(&next.insn) => MergedInstructionKind::Plain,
            None => break, // <- reads or writes flags, or leaves
        };

        instructions.push(kind);
        if kind != MergedInstructionKind::Plain {
            merged_len = instructions.len();
        }
    }

    // Plain instructions after the last eflags reader stay out.
    instructions.truncate(merged_len);
    if instructions.is_empty() {
        return None;
    }

    // Step 3: check which flags each path needs.
    //
    // jnz differ      ; taken:     CF    <- differ: sbb reads CF
    //                 ; not taken: none  <- cmp ecx, edx overwrites all flags
    //
    // And a bit of eflags elimination...
    //
    // Basically, a path only needs flags written only if it reads a flag that
    // is changed by the compare (compare.insn).
    //
    // Example:
    //
    //     add eax, ebx    ; sets CF
    //     dec ecx         ; the compare: changes ZF, leaves CF alone
    //     jnz next        ; taken path reads CF at `next`
    //     ...
    // next:
    //     adc edx, 0      ; CF is still the value from `add`
    //
    // `dec` didn't change CF, so the taken path needs nothing written.
    let compare_written = EFlagEffects::of(&compare.insn).writes;
    let first = index + 1 + usize::from(x87_pop); // first instruction after cmp+pop
    for (op_index, kind) in (first..).zip(instructions.iter_mut()) {
        // Note: First edge is the taken one.
        if let MergedInstructionKind::Jump(taken) = kind {
            let needed = *live.on_edges(start + op_index).first()?;
            *taken = flags_to_write(needed, compare_written);
        }
    }

    // uuurghhhh... my brain's meltingggggggg.
    // hewp hwep hewp
    let last = first + instructions.len() - 1;
    let needed = *live.on_edges(start + last).last()?;
    let end_flags = flags_to_write(needed, compare_written);
    let mut exits = instructions.iter().filter_map(|kind| match kind {
        MergedInstructionKind::Jump(flags) => Some(*flags),
        _ => None,
    });

    let mergeable = if can_write_flags {
        end_flags.is_empty() || exits.any(|flags| flags.is_empty())
    } else {
        end_flags.is_empty() && exits.all(|flags| flags.is_empty())
    };

    mergeable.then_some(Plan {
        compare: BlockOpIndex::new(index),
        x87_pop,
        instructions,
        end_flags,
    })
}

/// The flags a path needs the compare to write.
///
/// # Arguments
///
/// - `needed`: flags the path reads later.
/// - `compare_written`: flags the compare changes.
///
/// # Returns
///
/// `needed` if the compare changes any of them.
/// Empty otherwise.
///
/// # Remarks
///
/// The flag helpers keep flags the compare doesn't change, so writing all of
/// `needed` is safe.
fn flags_to_write(needed: CpuFlag, compare_written: CpuFlag) -> CpuFlag {
    if needed.intersects(compare_written) {
        needed
    } else {
        CpuFlag::empty()
    }
}

/// Whether `insn` can sit between the compare and its jumps in a [`MergedCmpJmp`].
///
/// # Examples
///
/// ```text
/// cmp eax, ebx    ; the compare
/// mov ecx, edx    ; <- can `insn` live in here !?
/// jnz differ      ; the jump
/// ```
fn is_plain(insn: &Insn) -> bool {
    !touches_flags(insn) && !leaves_block(insn)
}

/// Whether `insn` is `fstp st0`, which pops the x87 stack.
fn is_pop_st0(insn: &Insn) -> bool {
    insn.mnemonic() == Mnemonic::FSTP && register(&insn.operands()[0]) == Some(Register::ST0)
}

/// Whether `insn` is a jump, call, return, interrupt or system instruction.
fn leaves_block(insn: &Insn) -> bool {
    matches!(
        insn.instruction().meta.category,
        InstructionCategory::COND_BR
            | InstructionCategory::UNCOND_BR
            | InstructionCategory::CALL
            | InstructionCategory::RET
            | InstructionCategory::INTERRUPT
            | InstructionCategory::SYSCALL
            | InstructionCategory::SYSRET
            | InstructionCategory::SYSTEM
    )
}

/// Whether `insn` reads or writes any arithmetic flag, even only sometimes.
fn touches_flags(insn: &Insn) -> bool {
    let decoded = insn.instruction().cpu_flags;
    let might_touch =
        decoded.tested | decoded.modified | decoded.set_0 | decoded.set_1 | decoded.undefined;

    let effects = EFlagEffects::of(insn);

    might_touch.intersects(ALL_ARITHMETIC_FLAGS)
        || !effects.reads.is_empty()
        || !effects.writes.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use core::slice;
    use rstest::rstest;

    const PLAIN: MergedInstructionKind = MergedInstructionKind::Plain;
    const SELECT: MergedInstructionKind = MergedInstructionKind::Select;

    /// A jump that writes no flags when taken.
    const JUMP: MergedInstructionKind = MergedInstructionKind::Jump(CpuFlag::empty());

    /// The kind of each instruction in each merged op, in order.
    fn kinds(model: &FunctionModel) -> Vec<MergedInstructionKind> {
        model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .filter_map(|op| match op {
                Op::MergedCmpJmp(merged) => Some(merged),
                _ => None,
            })
            .flat_map(|merged| merged.instructions.iter().map(|insn| insn.kind))
            .collect()
    }

    #[rstest]
    #[case::signed(
        &[
            0x66, 0x83, 0xF8, 0xFF, //     cmp  ax, -1     ; 16-bit
            0x7C, 0,                //     jl   .end       ; signed
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[JUMP],
    )]
    #[case::logic_below(
        &[
            0x21, 0xD8,             //     and  eax, ebx
            0x72, 0,                //     jb   .end       ; CF is always clear: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::inc_below(
        &[
            0x40,                   //     inc  eax
            0x72, 0,                //     jb   .end       ; reads CF from before inc: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::add_signed(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x7C, 0,                //     jl   .end       ; needs OF: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::sahf_signed(
        &[
            0x9E,                   //     sahf
            0x7C, 0,                //     jl   .end       ; needs OF, which sahf keeps: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::overflow(
        &[
            0x39, 0xD8,             //     cmp  eax, ebx
            0x70, 0,                //     jo   .end       ; no direct test: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::shift_by_cl(
        &[
            0xD3, 0xE0,             //     shl  eax, cl    ; cl may be 0: no flags written
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::locked(
        &[
            0xF0, 0xFF, 0x0E,       //     lock dec [esi]  ; must stay atomic
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::stack_pointer(
        &[
            0x83, 0xEC, 8,          //     sub  esp, 8     ; stack passes track esp
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    // Step 2: look at what follows.
    #[case::fcomip_pop(
        &[
            0xDF, 0xF1,             //     fcomip st0, st1
            0xDD, 0xD8,             //     fstp st0        ; merged as the pop
            0x72, 0,                //     jb   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[JUMP],
    )]
    #[case::setcc(
        &[
            0x39, 0xD8,             // cmp  eax, ebx
            0x0F, 0x94, 0xC1,       // setz cl
            0x39, 0xD1,             // cmp  ecx, edx
            0xC3,                   // ret
        ],
        &[SELECT],
    )]
    #[case::cmov_then_jump(
        &[
            0x39, 0xD8,             //     cmp   eax, ebx
            0x0F, 0x4C, 0xCA,       //     cmovl ecx, edx
            0xBA, 1, 0, 0, 0,       //     mov   edx, 1
            0x74, 0,                //     jz    .end
                                    // .end:
            0x39, 0xD1,             //     cmp   ecx, edx
            0xC3,                   //     ret
        ],
        &[SELECT, PLAIN, JUMP],
    )]
    #[case::trailing_plain(
        &[
            0x39, 0xD8,             //     cmp  eax, ebx
            0x74, 8,                //     jz   .end
            0x0F, 0x92, 0xC1,       //     setb cl
            0xBA, 1, 0, 0, 0,       //     mov  edx, 1     ; after the last eflags reader: stays out
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[JUMP, SELECT],
    )]
    #[case::flag_writer(
        &[
            0x85, 0xC0,             //     test eax, eax
            0x83, 0xD1, 1,          //     adc  ecx, 1     ; reads and writes flags: stop
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::maybe_flags(
        &[
            0x85, 0xC0,             //     test eax, eax
            0xD3, 0xC1,             //     rol  ecx, cl    ; writes flags unless cl is 0
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::call(
        &[
            0x85, 0xC0,             //     test eax, eax
            0xE8, 0, 0, 0, 0,       //     call .next      ; call: stop
                                    // .next:
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::label(
        &[
            0xEB, 2,                //     jmp  .jz
            0x85, 0xC0,             //     test eax, eax
                                    // .jz:
            0x74, 0,                //     jz   .end       ; jump target: stop
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[],
    )]
    // Step 3: check which flags each path needs.
    #[case::one_side(
        &[
            0x83, 0xF8, 0x20,       //     cmp  eax, 32
            0x74, 3,                //     jz   .ret       ; taken: caller sees the flags
            0x39, 0xD1,             //     cmp  ecx, edx   ; not taken: none
            0xC3,                   //     ret
                                    // .ret:
            0xC3,                   //     ret
        ],
        &[MergedInstructionKind::Jump(ALL_ARITHMETIC_FLAGS)],
    )]
    #[case::taken_flags(
        &[
            0x39, 0xD8,             //     cmp  eax, ebx
            0x7C, 5,                //     jl   .setz      ; taken: needs ZF
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
                                    // .setz:
            0x0F, 0x94, 0xC0,       //     setz al
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[MergedInstructionKind::Jump(CpuFlag::ZF), JUMP],
    )]
    #[case::live_after_return(
        &[
            0x83, 0xF8, 0x20,       //     cmp  eax, 32
            0x74, 1,                //     jz   .ret       ; both paths: caller sees the flags
            0xC3,                   //     ret
                                    // .ret:
            0xC3,                   //     ret
        ],
        &[],
    )]
    #[case::external_target(
        &[
            0x83, 0xF8, 0x20,       // cmp  eax, 32
            0x74, 0x10,             // jz   0x1015         ; outside the function: all flags
            0xC3,                   // ret                 ; caller sees the flags
        ],
        &[],
    )]
    #[case::select_live(
        &[
            0x39, 0xD8,             // cmp  eax, ebx
            0x0F, 0x94, 0xC1,       // setz cl
            0xC3,                   // ret                 ; caller sees the flags
        ],
        &[],
    )]
    #[case::kept_carry(
        &[
            0xF9,                   //     stc
                                    // .loop:
            0x11, 0xD8,             //     adc  eax, ebx   ; reads CF
            0x49,                   //     dec  ecx        ; keeps CF
            0x75, 0xFB,             //     jnz  .loop      ; taken: CF, which dec didn't change
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        &[JUMP],
    )]
    #[case::float_one_side(
        &[
            0x0F, 0x2F, 0xC1,       //     comiss xmm0, xmm1
            0x72, 3,                //     jb   .setz      ; taken: needs flags
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
                                    // .setz:
            0x0F, 0x94, 0xC0,       //     setz al
            0xC3,                   //     ret
        ],
        &[],
    )]
    fn run_should_merge_eflags_readers(
        #[case] code: &[u8],
        #[case] expected: &[MergedInstructionKind],
    ) {
        // Arrange
        let mut model = model_of(code);

        // Act
        run(slice::from_mut(&mut model), &FlagBoundaries::default());

        // Assert
        assert_eq!(kinds(&model), expected);
    }
}
