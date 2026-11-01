//! Turns what a jump or select checks into a C++ test, and writes flags.

use super::super::prelude::*;
use super::IntegerCompare;
use crate::codegen::function::model::Condition;
use crate::decode::CpuFlag;

impl IntegerCompare {
    /// The C++ test for `condition`, like `ctx.eax.u32 == 0x20u`.
    pub(super) fn test(&self, condition: Condition) -> String {
        let IntegerCompare {
            lhs,
            rhs,
            result,
            bits,
            ..
        } = self;
        let signed = |value: &str| format!("static_cast<int{bits}_t>({value})");
        match condition {
            // static_cast<int32_t>(ctx.eax.u32) < static_cast<int32_t>(ctx.ebx.u32)
            Condition::Compare(operator, true) => {
                format!("{} {operator} {}", signed(lhs), signed(rhs))
            }
            // ctx.eax.u32 < ctx.ebx.u32
            Condition::Compare(operator, false) => format!("{lhs} {operator} {rhs}"),
            // static_cast<int32_t>(ctx.eax.u32 & 0x80u) < 0
            Condition::CompareToZero(operator, true) => format!("{} {operator} 0", signed(result)),
            // ctx.eax.u32 == 0
            Condition::CompareToZero(operator, false) => format!("{result} {operator} 0"),
            // static_cast<int32_t>(ctx.eax.u32) < 0
            Condition::Sign(set) => {
                let operator = if set { "<" } else { ">=" };
                format!("{} {operator} 0", signed(result))
            }
            // __builtin_parity(static_cast<uint8_t>(ctx.eax.u32)) == 0
            Condition::Parity(set) => {
                let operator = if set { "==" } else { "!=" };
                let low_byte = if *bits == 8 {
                    result.clone()
                } else {
                    format!("static_cast<uint8_t>({result})")
                };
                format!("__builtin_parity({low_byte}) {operator} 0")
            }
            // ctx.eax.u32 < old  (the sum wrapped)
            Condition::Carry(set) => {
                let operator = if set { "<" } else { ">=" };
                format!("{result} {operator} {lhs}")
            }
            Condition::Flags(..) => unreachable!("integer compares have integer conditions"),
        }
    }

    /// The C++ that writes the `needed` flags, as the compare would.
    ///
    /// # Examples
    ///
    /// ```text
    /// ctx.eflags.update_compare_flags_U32<0x40>(ctx.eax.u32, ctx.ebx.u32);
    /// ctx.eflags.update_flags_dec_U32<0x1>(old, ctx.ecx.u32);
    /// ```
    pub(super) fn flag_write(&self, needed: CpuFlag) -> String {
        let IntegerCompare {
            mnemonic,
            lhs,
            rhs,
            result,
            bits,
        } = self;
        let suffix = operand_size_name(*bits);
        let mask = format!("<0x{:X}>", needed.bits());
        let name = mnemonic.static_string().expect("mnemonics have names");
        match *mnemonic {
            Mnemonic::CMP => {
                format!("ctx.eflags.update_compare_flags_{suffix}{mask}({lhs}, {rhs});")
            }
            Mnemonic::TEST => format!("ctx.eflags.update_test_flags_{suffix}{mask}({result});"),
            // `update_flags_logic` picks the width from `result`'s type.
            Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR => {
                format!("ctx.eflags.update_flags_logic{mask}({result});")
            }
            Mnemonic::ADD | Mnemonic::SUB => {
                format!("ctx.eflags.update_flags_{name}_{suffix}{mask}({lhs}, {rhs}, {result});")
            }
            Mnemonic::INC | Mnemonic::DEC | Mnemonic::NEG => {
                format!("ctx.eflags.update_flags_{name}_{suffix}{mask}({lhs}, {result});")
            }
            // The shift helpers take the width in bytes.
            Mnemonic::SHL | Mnemonic::SHR | Mnemonic::SAR => {
                let bytes = bits / 8;
                format!("ctx.eflags.update_flags_{name}{mask}({lhs}, {rhs}, {result}, {bytes});")
            }
            _ => unreachable!("merging accepts only these"),
        }
    }
}

/// The C++ test after a float compare, whose merged test is `taken`.
pub(super) fn float_test(taken: &str, condition: Condition) -> String {
    match condition {
        Condition::Flags(_, true) => taken.to_owned(),
        _ => format!("!{taken}"),
    }
}

/// The C++ test of the flags held in `value`, like `ah` after `sahf`.
///
/// # Examples
///
/// ```text
/// (ctx.eax.u8_hi & X86EFlags::ZF) != 0
/// ```
pub(super) fn flags_test(value: &str, condition: Condition) -> String {
    let Condition::Flags(flags, set) = condition else {
        unreachable!("jumps and selects after sahf and float compares test flags")
    };
    let operator = if set { "!=" } else { "==" };
    format!("({value} & {}) {operator} 0", flag_names(flags))
}

/// `flags` as C++, like `(X86EFlags::CF | X86EFlags::ZF)`.
fn flag_names(flags: CpuFlag) -> String {
    let names: Vec<&str> = [
        (CpuFlag::CF, "X86EFlags::CF"),
        (CpuFlag::PF, "X86EFlags::PF"),
        (CpuFlag::ZF, "X86EFlags::ZF"),
        (CpuFlag::SF, "X86EFlags::SF"),
    ]
    .into_iter()
    .filter(|(flag, _)| flags.contains(*flag))
    .map(|(_, name)| name)
    .collect();
    match names.as_slice() {
        [name] => (*name).to_owned(),
        _ => format!("({})", names.join(" | ")),
    }
}
