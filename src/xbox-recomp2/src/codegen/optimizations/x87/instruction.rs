//! How each x87 instruction moves the stack.

use super::Depth;
use crate::codegen::function::has_emitter;
use crate::codegen::optimizations::helpers::instruction::fpu::{is_x87, visible_registers};
use crate::decode::{Insn, Mnemonic, Mnemonic::*, Register, RegisterClass};

/// What one instruction does to the x87 stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StackEffect {
    /// Values it needs on the stack before it runs, like 2 for `fadd st0, st1`.
    pub reads: Depth,

    /// How it moves the depth.
    pub change: DepthChange,
}

/// How an instruction moves the x87 depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DepthChange {
    /// Adds this many values; negative pops.
    By(i8),

    /// Empties the stack, like `fninit`.
    Reset,
}

/// Whether `mnemonic` touches state the local copy doesn't carry.
///
/// `fldcw` and `fxam` aren't here: they work fine on a copy.
pub(super) fn blocks_local_copy(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        FNINIT
            | FNSAVE
            | FRSTOR
            | FLDENV
            | FNSTENV
            | FFREE
            | FFREEP
            | FINCSTP
            | FDECSTP
            | FXSAVE
            | FXRSTOR
            | EMMS
            | FEMMS
    )
}

/// The `i` in `ffree st(i)`.
/// [`None`] for any other instruction.
pub(super) fn ffree_slot(insn: &Insn) -> Option<u8> {
    if insn.mnemonic() != FFREE {
        return None;
    }

    let reg = visible_registers(insn).next()?;
    if reg.class() != RegisterClass::X87 {
        return None;
    }

    Some((reg as u16 - Register::ST0 as u16) as u8)
}

/// Whether `insn` moves the depth in a way we can't count.
pub(super) fn is_uncountable(insn: &Insn, fast_trig: bool) -> bool {
    let mnemonic = insn.mnemonic();
    let uncountable = matches!(
        mnemonic,
        FFREE | FFREEP | FINCSTP | FDECSTP | FRSTOR | FLDENV | FXRSTOR
    );
    let trig = !fast_trig && matches!(mnemonic, FPTAN | FSINCOS);

    // We can't emit it, so it does nothing in the output.
    let moves = is_x87(insn) || depth_change(mnemonic) != DepthChange::By(0);
    let not_emitted = moves && !has_emitter(mnemonic);
    uncountable || trig || not_emitted
}

/// What `insn` does to the x87 stack.
/// Nothing, if it isn't x87.
pub(super) fn stack_effect(insn: &Insn) -> StackEffect {
    let mnemonic = insn.mnemonic();
    let change = depth_change(mnemonic);
    let reads = if is_x87(insn) { values_read(insn) } else { 0 };
    StackEffect { reads, change }
}

fn depth_change(mnemonic: Mnemonic) -> DepthChange {
    let by = match mnemonic {
        FLD | FILD | FLD1 | FLDZ | FLDPI | FLDL2E | FLDL2T | FLDLG2 | FLDLN2 | FBLD | FPTAN
        | FSINCOS | FXTRACT => 1,
        FSTP | FISTP | FISTTP | FBSTP | FADDP | FSUBP | FSUBRP | FMULP | FDIVP | FDIVRP | FCOMP
        | FUCOMP | FCOMIP | FUCOMIP | FPATAN | FYL2X | FYL2XP1 | FICOMP | FFREEP => -1,
        FCOMPP | FUCOMPP => -2,
        FNINIT | FNSAVE | EMMS | FEMMS => return DepthChange::Reset,
        _ => 0,
    };

    DepthChange::By(by)
}

/// Values `insn` needs on the stack before it runs.
fn values_read(insn: &Insn) -> Depth {
    let mnemonic = insn.mnemonic();

    // The highest `st(i)` it names, read or written.
    let highest = visible_registers(insn)
        .filter(|reg| reg.class() == RegisterClass::X87)
        .map(|reg| (reg as u16 - Register::ST0 as u16) as u8 + 1)
        .max()
        .unwrap_or(0);

    match mnemonic {
        // Loads and control stuff, no reads.
        FILD | FLD1 | FLDZ | FLDPI | FLDL2E | FLDL2T | FLDLG2 | FLDLN2 | FBLD | FNSTSW | FNSTCW
        | FNSTENV | FNSAVE | FLDCW | FLDENV | FRSTOR | FWAIT | FNCLEX | FNINIT | FFREE | FFREEP
        | FNOP | FINCSTP | FDECSTP => 0,

        // `fld st(i)` needs st(i), `fld [mem]` needs nothing.
        FLD => highest,

        // Always st0 and st1, e.g. plain `faddp` is `faddp st1, st0`.
        FPATAN | FYL2X | FYL2XP1 | FSCALE | FPREM | FPREM1 | FCOMPP | FUCOMPP | FADDP | FSUBP
        | FSUBRP | FMULP | FDIVP | FDIVRP => highest.max(2),

        // The rest need st0.
        _ => highest.max(1),
    }
}
