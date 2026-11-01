//! Turns x86 instructions into C++.
//!
//! Each instruction has its own file with an `emit` function, grouped into
//! folders by family. [`lookup`] picks the right one for a mnemonic.

use super::Emitter;
pub(in crate::codegen::function::write) use control_transfer::{
    call::emit as emit_call, ret::emit as emit_return,
};
use zydis::Mnemonic;

mod addressing;
pub(in crate::codegen) use addressing::is_supported as supports_addressing;
mod arithmetic;
mod bitwise;
mod compare;
mod control_transfer;
mod data_transfer;
mod flag_control;
pub(in crate::codegen::function::write) mod merged_cmp_jmp;
mod mmx;
mod prelude;
mod sse;
mod string;
mod system;
mod x87;

/// A function that writes the C++ for one instruction.
type EmitterFn = fn(&mut Emitter<'_>);

/// Finds the emitter for an instruction, or `None` if it isn't supported.
pub(in crate::codegen::function::write) fn lookup(
    mnemonic: Mnemonic,
) -> Option<impl Fn(&mut Emitter<'_>) + Copy> {
    let emitter = emitter_for(mnemonic)?;
    Some(move |ctx: &mut Emitter<'_>| {
        if addressing::is_supported(ctx.insn) {
            emitter(ctx);
        } else {
            ctx.missing_form();
        }
    })
}

fn emitter_for(mnemonic: Mnemonic) -> Option<EmitterFn> {
    let emitter: EmitterFn = match mnemonic {
        Mnemonic::AAA => arithmetic::aaa::emit,
        Mnemonic::AAD => arithmetic::aad::emit,
        Mnemonic::AAS => arithmetic::aas::emit,
        Mnemonic::ADC => arithmetic::adc::emit,
        Mnemonic::ADD => arithmetic::add::emit,
        Mnemonic::ADDPS => sse::addps::emit,
        Mnemonic::ADDSS => sse::addss::emit,
        Mnemonic::AND => bitwise::and::emit,
        Mnemonic::ANDPS => sse::andps::emit,
        Mnemonic::BSF => bitwise::bsf::emit,
        Mnemonic::CDQ => arithmetic::cdq::emit,
        Mnemonic::CLD => flag_control::cld::emit,
        Mnemonic::CLI => flag_control::cli::emit,
        Mnemonic::CMOVB => data_transfer::cmovb::emit,
        Mnemonic::CMOVBE => data_transfer::cmovbe::emit,
        Mnemonic::CMOVL => data_transfer::cmovl::emit,
        Mnemonic::CMOVLE => data_transfer::cmovle::emit,
        Mnemonic::CMOVNB => data_transfer::cmovnb::emit,
        Mnemonic::CMOVNBE => data_transfer::cmovnbe::emit,
        Mnemonic::CMOVNL => data_transfer::cmovnl::emit,
        Mnemonic::CMOVNLE => data_transfer::cmovnle::emit,
        Mnemonic::CMOVNO => data_transfer::cmovno::emit,
        Mnemonic::CMOVNP => data_transfer::cmovnp::emit,
        Mnemonic::CMOVNS => data_transfer::cmovns::emit,
        Mnemonic::CMOVNZ => data_transfer::cmovnz::emit,
        Mnemonic::CMOVO => data_transfer::cmovo::emit,
        Mnemonic::CMOVP => data_transfer::cmovp::emit,
        Mnemonic::CMOVS => data_transfer::cmovs::emit,
        Mnemonic::CMOVZ => data_transfer::cmovz::emit,
        Mnemonic::CMP => arithmetic::cmp::emit,
        Mnemonic::CMPSB => string::cmpsb::emit,
        Mnemonic::CMPSD => string::cmpsd::emit,
        Mnemonic::CMPSS => sse::cmpss::emit,
        Mnemonic::CMPSW => string::cmpsw::emit,
        Mnemonic::CMPXCHG => data_transfer::cmpxchg::emit,
        Mnemonic::COMISS => sse::comiss::emit,
        Mnemonic::CVTPS2PI => sse::cvtps2pi::emit,
        Mnemonic::CVTSI2SS => sse::cvtsi2ss::emit,
        Mnemonic::CVTSS2SI => sse::cvtss2si::emit,
        Mnemonic::CVTTSS2SI => sse::cvttss2si::emit,
        Mnemonic::CWDE => arithmetic::cwde::emit,
        Mnemonic::DEC => arithmetic::dec::emit,
        Mnemonic::DIV => arithmetic::div::emit,
        Mnemonic::DIVSS => sse::divss::emit,
        Mnemonic::EMMS => mmx::emms::emit,
        Mnemonic::F2XM1 => x87::f2xm1::emit,
        Mnemonic::FABS => x87::fabs::emit,
        Mnemonic::FADD => x87::fadd::emit,
        Mnemonic::FADDP => x87::faddp::emit,
        Mnemonic::FCHS => x87::fchs::emit,
        Mnemonic::FCOM => x87::fcom::emit,
        Mnemonic::FCOMIP => x87::fcomip::emit,
        Mnemonic::FCOMP => x87::fcomp::emit,
        Mnemonic::FCOMPP => x87::fcompp::emit,
        Mnemonic::FCOS => x87::fcos::emit,
        Mnemonic::FDIV => x87::fdiv::emit,
        Mnemonic::FDIVP => x87::fdivp::emit,
        Mnemonic::FDIVR => x87::fdivr::emit,
        Mnemonic::FDIVRP => x87::fdivrp::emit,
        Mnemonic::FFREE => x87::ffree::emit,
        Mnemonic::FIADD => x87::fiadd::emit,
        Mnemonic::FIDIV => x87::fidiv::emit,
        Mnemonic::FILD => x87::fild::emit,
        Mnemonic::FIMUL => x87::fimul::emit,
        Mnemonic::FISTP => x87::fistp::emit,
        Mnemonic::FLD => x87::fld::emit,
        Mnemonic::FLD1 => x87::fld1::emit,
        Mnemonic::FLDCW => x87::fldcw::emit,
        Mnemonic::FLDL2E => x87::fldl2e::emit,
        Mnemonic::FLDLG2 => x87::fldlg2::emit,
        Mnemonic::FLDLN2 => x87::fldln2::emit,
        Mnemonic::FLDPI => x87::fldpi::emit,
        Mnemonic::FLDZ => x87::fldz::emit,
        Mnemonic::FMUL => x87::fmul::emit,
        Mnemonic::FMULP => x87::fmulp::emit,
        Mnemonic::FNCLEX => x87::fnclex::emit,
        Mnemonic::FNSAVE => x87::fnsave::emit,
        Mnemonic::FNSTCW => x87::fnstcw::emit,
        Mnemonic::FNSTSW => x87::fnstsw::emit,
        Mnemonic::FPATAN => x87::fpatan::emit,
        Mnemonic::FPREM => x87::fprem::emit,
        Mnemonic::FPREM1 => x87::fprem1::emit,
        Mnemonic::FPTAN => x87::fptan::emit,
        Mnemonic::FRNDINT => x87::frndint::emit,
        Mnemonic::FRSTOR => x87::frstor::emit,
        Mnemonic::FSCALE => x87::fscale::emit,
        Mnemonic::FSIN => x87::fsin::emit,
        Mnemonic::FSINCOS => x87::fsincos::emit,
        Mnemonic::FSQRT => x87::fsqrt::emit,
        Mnemonic::FST => x87::fst::emit,
        Mnemonic::FSTP => x87::fstp::emit,
        Mnemonic::FSUB => x87::fsub::emit,
        Mnemonic::FSUBP => x87::fsubp::emit,
        Mnemonic::FSUBR => x87::fsubr::emit,
        Mnemonic::FSUBRP => x87::fsubrp::emit,
        Mnemonic::FTST => x87::ftst::emit,
        Mnemonic::FUCOMIP => x87::fucomip::emit,
        Mnemonic::FUCOMP => x87::fucomp::emit,
        Mnemonic::FUCOMPP => x87::fucompp::emit,
        Mnemonic::FWAIT => x87::fwait::emit,
        Mnemonic::FXAM => x87::fxam::emit,
        Mnemonic::FXCH => x87::fxch::emit,
        Mnemonic::FYL2X => x87::fyl2x::emit,
        Mnemonic::IDIV => arithmetic::idiv::emit,
        Mnemonic::IMUL => arithmetic::imul::emit,
        Mnemonic::IN => system::r#in::emit,
        Mnemonic::INC => arithmetic::inc::emit,
        Mnemonic::INT => control_transfer::int::emit,
        Mnemonic::INT3 => control_transfer::int3::emit,
        Mnemonic::JB => control_transfer::jb::emit,
        Mnemonic::JBE => control_transfer::jbe::emit,
        Mnemonic::JCXZ | Mnemonic::JECXZ => control_transfer::jecxz::emit,
        Mnemonic::JL => control_transfer::jl::emit,
        Mnemonic::JLE => control_transfer::jle::emit,
        Mnemonic::JMP => control_transfer::jmp::emit,
        Mnemonic::JNB => control_transfer::jnb::emit,
        Mnemonic::JNBE => control_transfer::jnbe::emit,
        Mnemonic::JNL => control_transfer::jnl::emit,
        Mnemonic::JNLE => control_transfer::jnle::emit,
        Mnemonic::JNO => control_transfer::jno::emit,
        Mnemonic::JNP => control_transfer::jnp::emit,
        Mnemonic::JNS => control_transfer::jns::emit,
        Mnemonic::JNZ => control_transfer::jnz::emit,
        Mnemonic::JO => control_transfer::jo::emit,
        Mnemonic::JP => control_transfer::jp::emit,
        Mnemonic::JS => control_transfer::js::emit,
        Mnemonic::JZ => control_transfer::jz::emit,
        Mnemonic::LAHF => flag_control::lahf::emit,
        Mnemonic::LDMXCSR => sse::ldmxcsr::emit,
        Mnemonic::LEA => data_transfer::lea::emit,
        Mnemonic::LEAVE => data_transfer::leave::emit,
        Mnemonic::LOOPE => control_transfer::loope::emit,
        Mnemonic::MAXPS => sse::maxps::emit,
        Mnemonic::MINPS => sse::minps::emit,
        Mnemonic::MOV => data_transfer::mov::emit,
        Mnemonic::MOVAPS => sse::movaps::emit,
        Mnemonic::MOVD => mmx::movd::emit,
        Mnemonic::MOVHLPS => sse::movhlps::emit,
        Mnemonic::MOVHPS => sse::movhps::emit,
        Mnemonic::MOVLHPS => sse::movlhps::emit,
        Mnemonic::MOVLPS => sse::movlps::emit,
        Mnemonic::MOVNTPS => sse::movntps::emit,
        Mnemonic::MOVNTQ => mmx::movntq::emit,
        Mnemonic::MOVQ => mmx::movq::emit,
        Mnemonic::MOVSB => string::movsb::emit,
        Mnemonic::MOVSD => string::movsd::emit,
        Mnemonic::MOVSS => sse::movss::emit,
        Mnemonic::MOVSW => string::movsw::emit,
        Mnemonic::MOVSX => data_transfer::movsx::emit,
        Mnemonic::MOVUPS => sse::movups::emit,
        Mnemonic::MOVZX => data_transfer::movzx::emit,
        Mnemonic::MUL => arithmetic::mul::emit,
        Mnemonic::MULPS => sse::mulps::emit,
        Mnemonic::MULSS => sse::mulss::emit,
        Mnemonic::NEG => arithmetic::neg::emit,
        Mnemonic::NOP => system::nop::emit,
        Mnemonic::NOT => bitwise::not::emit,
        Mnemonic::OR => bitwise::or::emit,
        Mnemonic::OUT => system::out::emit,
        Mnemonic::PACKSSDW => mmx::packssdw::emit,
        Mnemonic::PACKUSWB => mmx::packuswb::emit,
        Mnemonic::PADDW => mmx::paddw::emit,
        Mnemonic::PAVGB => mmx::pavgb::emit,
        Mnemonic::PEXTRW => mmx::pextrw::emit,
        Mnemonic::POP => data_transfer::pop::emit,
        Mnemonic::PREFETCHNTA => system::nop::emit,
        Mnemonic::PREFETCHT0 => system::prefetcht0::emit,
        Mnemonic::PSHUFW => mmx::pshufw::emit,
        Mnemonic::PSRAW => mmx::psraw::emit,
        Mnemonic::PSRLQ => mmx::psrlq::emit,
        Mnemonic::PUNPCKHDQ => mmx::punpckhdq::emit,
        Mnemonic::PUNPCKHWD => mmx::punpckhwd::emit,
        Mnemonic::PUNPCKLDQ => mmx::punpckldq::emit,
        Mnemonic::PUNPCKLWD => mmx::punpcklwd::emit,
        Mnemonic::PUSH => data_transfer::push::emit,
        Mnemonic::PUSHFD => data_transfer::pushfd::emit,
        Mnemonic::RCR => bitwise::rcr::emit,
        Mnemonic::RDTSC => system::rdtsc::emit,
        Mnemonic::ROL => bitwise::rol::emit,
        Mnemonic::ROR => bitwise::ror::emit,
        Mnemonic::RSQRTSS => sse::rsqrtss::emit,
        Mnemonic::SAHF => flag_control::sahf::emit,
        Mnemonic::SAR => bitwise::sar::emit,
        Mnemonic::SBB => arithmetic::sbb::emit,
        Mnemonic::SCASB => string::scasb::emit,
        Mnemonic::SETB => data_transfer::setb::emit,
        Mnemonic::SETBE => data_transfer::setbe::emit,
        Mnemonic::SETL => data_transfer::setl::emit,
        Mnemonic::SETLE => data_transfer::setle::emit,
        Mnemonic::SETNB => data_transfer::setnb::emit,
        Mnemonic::SETNBE => data_transfer::setnbe::emit,
        Mnemonic::SETNL => data_transfer::setnl::emit,
        Mnemonic::SETNLE => data_transfer::setnle::emit,
        Mnemonic::SETNO => data_transfer::setno::emit,
        Mnemonic::SETNP => data_transfer::setnp::emit,
        Mnemonic::SETNS => data_transfer::setns::emit,
        Mnemonic::SETNZ => data_transfer::setnz::emit,
        Mnemonic::SETO => data_transfer::seto::emit,
        Mnemonic::SETP => data_transfer::setp::emit,
        Mnemonic::SETS => data_transfer::sets::emit,
        Mnemonic::SETZ => data_transfer::setz::emit,
        Mnemonic::SFENCE => system::sfence::emit,
        Mnemonic::SGDT => system::sgdt::emit,
        Mnemonic::SHL => bitwise::shl::emit,
        Mnemonic::SHLD => bitwise::shld::emit,
        Mnemonic::SHR => bitwise::shr::emit,
        Mnemonic::SHRD => bitwise::shrd::emit,
        Mnemonic::SHUFPS => sse::shufps::emit,
        Mnemonic::SLDT => system::sldt::emit,
        Mnemonic::SQRTSS => sse::sqrtss::emit,
        Mnemonic::STC => flag_control::stc::emit,
        Mnemonic::STD => flag_control::std::emit,
        Mnemonic::STI => flag_control::sti::emit,
        Mnemonic::STMXCSR => sse::stmxcsr::emit,
        Mnemonic::STOSB => string::stosb::emit,
        Mnemonic::STOSD => string::stosd::emit,
        Mnemonic::STOSW => string::stosw::emit,
        Mnemonic::SUB => arithmetic::sub::emit,
        Mnemonic::SUBPS => sse::subps::emit,
        Mnemonic::SUBSS => sse::subss::emit,
        Mnemonic::TEST => bitwise::test::emit,
        Mnemonic::UCOMISS => sse::ucomiss::emit,
        Mnemonic::UNPCKHPS => sse::unpckhps::emit,
        Mnemonic::UNPCKLPS => sse::unpcklps::emit,
        Mnemonic::WBINVD => system::wbinvd::emit,
        Mnemonic::XADD => arithmetic::xadd::emit,
        Mnemonic::XCHG => data_transfer::xchg::emit,
        Mnemonic::XLAT => string::xlat::emit,
        Mnemonic::XOR => bitwise::xor::emit,
        Mnemonic::XORPS => sse::xorps::emit,
        _ => return None,
    };
    Some(emitter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::function::model::ALL_ARITHMETIC_FLAGS;
    use crate::codegen::function::write::emit::{CodeWriter, FunctionState};
    use crate::codegen::optimizations::helpers::tests::{PlainPlatform, instruction};
    use crate::config::RecompilerConfig;
    use crate::executable::{ImportMap, SymbolTable};
    use rstest::rstest;

    fn form_missing(code: &[u8]) -> bool {
        let insn = instruction(code);
        let config = RecompilerConfig::default();
        let mut function_state = FunctionState::default();
        let mut out = CodeWriter::new();
        let mut emitter = Emitter {
            out: &mut out,
            base: 0x1000.into(),
            insn: &insn,
            needed_eflags: ALL_ARITHMETIC_FLAGS,
            symbols: &SymbolTable::default(),
            imports: &ImportMap::default(),
            config: &config,
            platform: &PlainPlatform,
            function_state: &mut function_state,
            switch_table: None,
            form_missing: false,
        };
        let emit = lookup(insn.mnemonic()).expect("instruction has an emitter");
        emit(&mut emitter);
        emitter.form_missing
    }

    // ── Operand forms ────────────────────────────────────────────────────

    #[rstest]
    #[case::sse2_unpack(&[0x66, 0x0F, 0x62, 0xC1], true)] // punpckldq xmm0, xmm1
    #[case::sse2_average(&[0x66, 0x0F, 0xE0, 0x0E], true)] // pavgb xmm1, [esi]
    #[case::mmx_unpack(&[0x0F, 0x62, 0x06], false)] // punpckldq mm0, [esi]
    #[case::gs_segment(&[0x65, 0x8B, 0x06], true)] // mov eax, gs:[esi]
    #[case::fs_with_register(&[0x64, 0x8B, 0x40, 0x04], true)] // mov eax, fs:[eax+4]
    #[case::address16_stack(&[0x67, 0x50], false)] // push eax, with a 16-bit address prefix
    #[case::address16(&[0x67, 0x8B, 0x00], true)] // mov eax, [bx+si]
    #[case::address16_lea(&[0x67, 0x8D, 0x00], false)] // lea eax, [bx+si]
    #[case::far_jump(&[0xEA, 0, 0x10, 0, 0, 0x08, 0], true)] // jmp far 0x8:0x1000
    #[case::control_register(&[0x0F, 0x20, 0xC0], true)] // mov eax, cr0
    fn lookup_should_report_missing_for_unsupported_forms(
        #[case] code: &[u8],
        #[case] missing: bool,
    ) {
        // Act
        let reported = form_missing(code);

        // Assert
        assert_eq!(reported, missing);
    }
}
