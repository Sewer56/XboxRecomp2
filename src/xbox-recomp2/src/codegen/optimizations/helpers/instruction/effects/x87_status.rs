//! Which x87 condition codes (C0-C3) an instruction reads and writes.
//!
//! See `skip-unused-x87-status.md` for more info.

use super::Effect;
use crate::decode::{Insn, Mnemonic::*};
use bitflags::bitflags;

bitflags! {
    /// The x87 condition codes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct ConditionCodes: u16 {
        const C0 = 1 << 8;
        const C1 = 1 << 9;
        const C2 = 1 << 10;
        const C3 = 1 << 14;
    }
}

/// The x87 condition codes an instruction reads and writes.
pub(in crate::codegen) type StatusEffect = Effect<ConditionCodes>;

impl StatusEffect {
    /// What `insn` does to the condition codes.
    pub(in crate::codegen) fn of(insn: &Insn) -> Self {
        const ALL: ConditionCodes = ConditionCodes::all();
        const C1: ConditionCodes = ConditionCodes::C1;
        const C2: ConditionCodes = ConditionCodes::C2;
        const NONE: ConditionCodes = ConditionCodes::empty();

        let (reads, writes) = match insn.mnemonic() {
            FNSTSW | FNSTENV | FNSAVE | FXSAVE => (ALL, NONE),

            // Their emitters write `sw &= ~C1`.
            FLD | FILD | FLD1 | FMUL | FADD | FSUB | FSUBR | FSUBRP | FDIV | FDIVR | FDIVRP
            | FABS | FCHS | FSQRT | FSCALE | FIADD | FIDIV | F2XM1 => (NONE, C1),

            // Trig: C0 and C3 are undefined, and we leave them as they were.
            FSIN | FSINCOS => (NONE, C1 | C2),
            FCOS | FPTAN => (NONE, C2),

            // Compares and `fxam` set all four.
            FCOM | FCOMP | FCOMPP | FUCOM | FUCOMP | FUCOMPP | FTST | FXAM | FICOM | FICOMP => {
                (NONE, ALL)
            }

            FPREM | FPREM1 => (NONE, C1 | C2),
            FNINIT | FRSTOR | FLDENV => (NONE, ALL),
            _ => (NONE, NONE),
        };

        Self { reads, writes }
    }
}
