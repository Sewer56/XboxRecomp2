//! Which eflags an instruction reads and writes.
//!
//! See `flag-effects.md` for more info.

use crate::codegen::function::model::ALL_ARITHMETIC_FLAGS;
use crate::codegen::function::supports_addressing;
use crate::decode::{
    CpuFlag, Insn, InstructionAttributes, InstructionCategory, Mnemonic, immediate,
};

/// Eflags an instruction reads and writes.
#[derive(Clone, Copy, Debug)]
pub(in crate::codegen) struct EFlagEffects {
    pub reads: CpuFlag,
    pub writes: CpuFlag,
}

impl EFlagEffects {
    /// Reads all eflags and writes none.
    pub(in crate::codegen) const fn barrier() -> Self {
        Self {
            reads: ALL_ARITHMETIC_FLAGS,
            writes: CpuFlag::empty(),
        }
    }

    /// Reads and writes no eflags.
    pub(in crate::codegen) const fn none() -> Self {
        Self {
            reads: CpuFlag::empty(),
            writes: CpuFlag::empty(),
        }
    }

    /// Gets the eflags `insn` reads and writes in the generated code.
    ///
    /// # Remarks
    ///
    /// Undefined flags count as written.
    ///
    /// For example, the Intel Manual states ZF is undefined after `mul`,
    /// so we count that as written.
    pub(in crate::codegen) fn of(insn: &Insn) -> Self {
        let decoded = insn.instruction();
        let flags = decoded.cpu_flags;

        // Unsupported mem addressing emits nothing.
        if !supports_addressing(insn) {
            return Self::barrier();
        }

        let reads = flags.tested & ALL_ARITHMETIC_FLAGS;
        let writes = match insn.mnemonic() {
            mnemonic if writes_all_flags(mnemonic) => ALL_ARITHMETIC_FLAGS,

            // x86 leaves flags undefined here.
            // (Note: we don't change 'em for perf go brr)
            Mnemonic::DIV | Mnemonic::IDIV | Mnemonic::INT3 | Mnemonic::IN | Mnemonic::OUT => {
                return Self::none();
            }

            // x86 leaves some flags undefined here, but our code in
            // core header sets them to 0 for perf.
            Mnemonic::AAA | Mnemonic::AAS | Mnemonic::AAD => ALL_ARITHMETIC_FLAGS,

            // Without REP, these always write all flags.
            Mnemonic::CMPSB | Mnemonic::CMPSW | Mnemonic::CMPSD | Mnemonic::SCASB
                if !insn.instruction().attributes.intersects(
                    InstructionAttributes::HAS_REPE | InstructionAttributes::HAS_REPNE,
                ) =>
            {
                ALL_ARITHMETIC_FLAGS
            }

            // With REP, ECX may be 0.
            Mnemonic::CMPSB | Mnemonic::CMPSW | Mnemonic::CMPSD | Mnemonic::SCASB => {
                CpuFlag::empty()
            }

            Mnemonic::ROL | Mnemonic::ROR | Mnemonic::RCR => rotate_writes(insn),
            Mnemonic::INC | Mnemonic::DEC => ALL_ARITHMETIC_FLAGS & !CpuFlag::CF,
            Mnemonic::STC => CpuFlag::CF,
            Mnemonic::SAHF => {
                return Self {
                    // Keeps OF and doesn't read the other flags.
                    reads: CpuFlag::empty(),
                    writes: ALL_ARITHMETIC_FLAGS & !CpuFlag::OF,
                };
            }
            mnemonic if is_shift(mnemonic) => shift_writes(insn),

            // Anything else that writes flags is a barrier.
            //
            // Note: This never runs for Shadow, so I never bothered handling more
            //       instructions here - Sewer.
            _ if (flags.modified | flags.set_0 | flags.set_1 | flags.undefined)
                .intersects(ALL_ARITHMETIC_FLAGS) =>
            {
                return Self::barrier();
            }
            _ => CpuFlag::empty(),
        };

        // Assume CALL and RET clobber all flags; as we don't expect them to
        // persist across MSVC-based compilations.
        if matches!(
            decoded.meta.category,
            InstructionCategory::CALL | InstructionCategory::RET
        ) || matches!(insn.mnemonic(), Mnemonic::INT)
        {
            return Self::barrier();
        }

        Self { reads, writes }
    }

    /// Whether the instruction can skip flags nobody reads.
    ///
    /// # Remarks
    ///
    /// Only use isns supported by [`Self::of`].
    pub(in crate::codegen) fn can_narrow(insn: &Insn) -> bool {
        let mnemonic = insn.mnemonic();
        (writes_all_flags(mnemonic) && !has_fixed_flags(mnemonic))
            || is_shift(mnemonic)
            || matches!(mnemonic, Mnemonic::INC | Mnemonic::DEC)
    }
}

/// Instructions whose helpers always compute all eflags.
fn has_fixed_flags(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::COMISS | Mnemonic::UCOMISS | Mnemonic::FCOMIP | Mnemonic::FUCOMIP
    )
}

fn is_shift(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::SHL | Mnemonic::SHR | Mnemonic::SAR | Mnemonic::SHLD | Mnemonic::SHRD
    )
}

/// CF and OF if the immediate count isn't 0.
fn rotate_writes(insn: &Insn) -> CpuFlag {
    let count = immediate(&insn.operands()[1]);
    if count.is_some_and(|imm| imm.value & 31 != 0) {
        CpuFlag::CF | CpuFlag::OF
    } else {
        CpuFlag::empty()
    }
}

/// All eflags if the immediate count isn't 0.
fn shift_writes(insn: &Insn) -> CpuFlag {
    let count_index = if matches!(insn.mnemonic(), Mnemonic::SHLD | Mnemonic::SHRD) {
        2
    } else {
        1
    };
    if immediate(&insn.operands()[count_index]).is_some_and(|imm| imm.value & 31 != 0) {
        ALL_ARITHMETIC_FLAGS
    } else {
        CpuFlag::empty()
    }
}

/// Instructions writing all eflags.
fn writes_all_flags(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::COMISS
            | Mnemonic::UCOMISS
            | Mnemonic::FCOMIP
            | Mnemonic::FUCOMIP
            | Mnemonic::BSF
            | Mnemonic::XADD
            | Mnemonic::CMPXCHG
            | Mnemonic::ADD
            | Mnemonic::ADC
            | Mnemonic::SUB
            | Mnemonic::SBB
            | Mnemonic::NEG
            | Mnemonic::CMP
            | Mnemonic::TEST
            | Mnemonic::AND
            | Mnemonic::OR
            | Mnemonic::XOR
            | Mnemonic::MUL
            | Mnemonic::IMUL
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::instruction;
    use rstest::rstest;

    #[rstest]
    #[case::sahf(
        &[0x9E],                             // sahf
        CpuFlag::empty(),                    // reads none
        ALL_ARITHMETIC_FLAGS & !CpuFlag::OF  // writes all but OF
    )]
    #[case::stc(
        &[0xF9],                             // stc
        CpuFlag::empty(),                    // reads none
        CpuFlag::CF                          // writes CF
    )]
    #[case::inc(
        &[0x40],                             // inc eax
        CpuFlag::empty(),                    // reads none
        ALL_ARITHMETIC_FLAGS & !CpuFlag::CF  // writes all but CF
    )]
    #[case::zero_shift(
        &[0xC1, 0xE0, 0x20],                 // shl eax,32
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none: count becomes 0
    )]
    #[case::variable_shift(
        &[0xD3, 0xE0],                       // shl eax,cl
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none: CL may be 0
    )]
    #[case::fs_memory(
        &[0x64, 0xA1, 0, 0, 0, 0],           // mov eax,fs:[0]
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none
    )]
    #[case::comiss(
        &[0x0F, 0x2F, 0xC1],                 // comiss xmm0,xmm1
        CpuFlag::empty(),                    // reads none
        ALL_ARITHMETIC_FLAGS                 // writes all
    )]
    #[case::fcomip(
        &[0xDF, 0xF1],                       // fcomip st0,st1
        CpuFlag::empty(),                    // reads none
        ALL_ARITHMETIC_FLAGS                 // writes all
    )]
    #[case::div(
        &[0xF7, 0xF1],                       // div ecx
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none
    )]
    #[case::fs_add(
        &[0x64, 0x01, 0x05, 0, 0, 0, 0],     // add fs:[0],eax
        ALL_ARITHMETIC_FLAGS,                // reads all
        CpuFlag::empty()                     // writes none: can't be emitted
    )]
    #[case::rcr(
        &[0xD1, 0xD8],                       // rcr eax,1
        CpuFlag::CF,                         // reads CF
        CpuFlag::CF | CpuFlag::OF            // writes CF, OF
    )]
    #[case::rotate_by_cl(
        &[0xD3, 0xC0],                       // rol eax,cl
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none: CL may be 0
    )]
    #[case::repe_cmpsb(
        &[0xF3, 0xA6],                       // repe cmpsb
        CpuFlag::ZF,                         // reads ZF
        CpuFlag::empty()                     // writes none: ECX may be 0
    )]
    #[case::cmpsb(
        &[0xA6],                             // cmpsb
        CpuFlag::empty(),                    // reads none
        ALL_ARITHMETIC_FLAGS                 // writes all
    )]
    #[case::aas(
        &[0x3F],                             // aas
        CpuFlag::AF,                         // reads AF
        ALL_ARITHMETIC_FLAGS                 // writes all
    )]
    #[case::port_in(
        &[0xEC],                             // in al,dx
        CpuFlag::empty(),                    // reads none
        CpuFlag::empty()                     // writes none
    )]
    fn of_should_report_flag_reads_and_writes(
        #[case] code: &[u8],
        #[case] reads: CpuFlag,
        #[case] writes: CpuFlag,
    ) {
        // Act
        let actual = EFlagEffects::of(&instruction(code));

        // Assert
        assert_eq!((actual.reads, actual.writes), (reads, writes));
    }
}
