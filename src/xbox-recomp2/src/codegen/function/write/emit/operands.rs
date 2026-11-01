//! Turns operands into C++ text, shared by all instruction emitters.
//!
//! - Registers: [`get_x86_reg`].
//! - Memory addresses: [`ea`].
//! - Whole operands: [`format_operand`].

use crate::decode::{immediate, memory, register};
pub(super) use crate::platform::operand_size_name;
use registers::get_addr_reg;
pub use registers::{
    get_x86_reg, get_x86_reg_float, is_mm0_to_7, is_st0_to_7, is_xmm0_to_7, mmx_index, st_index,
    xmm_index,
};
use zydis::ffi::{DecodedOperand, DecodedOperandKind, MemoryInfo};
use zydis::{MachineMode, MemoryOperandType, Register};

mod registers;

/// Machine mode for register class lookups.
pub(super) const MODE: MachineMode = MachineMode::LONG_COMPAT_32;

/// Returns the index register for a platform array's `{reg}` slot.
///
/// # Examples
///
/// ```text
/// array_subscript([esi*4], 32)  ->  "ctx.esi.u32"
/// array_subscript([esi*4], 16)  ->  "ctx.esi.u16"
/// array_subscript([eax], 32)    ->  "0"
/// array_subscript(eax, 32)      ->  "0"
/// ```
pub(crate) fn array_subscript(operand: &DecodedOperand, size: u16) -> String {
    let index = memory(operand).map(|mem| mem.index);
    match index {
        Some(index) if index as u16 != 0 => {
            format!("ctx.{}.u{size}", index.static_string().unwrap_or_default())
        }
        _ => "0".to_string(),
    }
}

/// Like [`ea`], but writes scale and displacement in decimal.
///
/// Gives `0` for an empty address.
///
/// # Examples
///
/// ```text
/// decode_memory_operand_no_parentheses([esi+eax*4+8])  ->  "ctx.esi.u32 + ctx.eax.u32 * 4 + 8"
/// decode_memory_operand_no_parentheses([ebx-0x10])     ->  "ctx.ebx.u32 - 16"
/// decode_memory_operand_no_parentheses([0x12345])      ->  "74565"
/// ```
pub(super) fn decode_memory_operand_no_parentheses(mem: &MemoryInfo) -> String {
    address_expression(mem, false)
}

/// Returns a memory operand's guest address.
///
/// A displacement on its own is a 32-bit guest address.
///
/// # Returns
///
/// - The address, or `0` if it's empty.
/// - An empty string if `operand` isn't memory.
///
/// # Examples
///
/// ```text
/// ea([esi+eax*4+8])  ->  "ctx.esi.u32 + ctx.eax.u32 * 0x4 + 0x8"
/// ea([ebx-0x10])     ->  "ctx.ebx.u32 - 0x10"
/// ea([0x12345])      ->  "0x12345"
/// ea(eax)            ->  ""
/// ```
pub(in super::super) fn ea(operand: &DecodedOperand) -> String {
    memory(operand).map_or_else(String::new, |mem| address_expression(mem, true))
}

/// Returns an operand as a C++ expression.
///
/// # Arguments
///
/// - `operand`: operand to read.
/// - `size_suffix`: `U8`, `U16` or `U32`, used for memory loads.
///
/// # Returns
///
/// - Registers: from [`get_x86_reg`].
/// - Immediates: from [`constant`], cut to the `size_suffix` width.
/// - Memory: `X86_LOAD_<size_suffix>(<ea>)`.
/// - Anything else: an empty string.
///
/// # Examples
///
/// ```text
/// format_operand(eax, "U32")       ->  "ctx.eax.u32"
/// format_operand(al, "U8")         ->  "ctx.eax.u8_lo"
/// format_operand(0x1f, "U32")      ->  "0x1Fu"
/// format_operand(-1, "U8")         ->  "0xFFu"
/// format_operand([esi+8], "U32")   ->  "X86_LOAD_U32(ctx.esi.u32 + 0x8)"
/// ```
pub(super) fn format_operand(operand: &DecodedOperand, size_suffix: &str) -> String {
    if register(operand).is_some() {
        return get_x86_reg(operand_reg_value(operand));
    }
    if let Some(imm) = immediate(operand) {
        let bits = match size_suffix {
            "U8" => 8,
            "U16" => 16,
            _ => 32,
        };
        return constant(imm.value, bits);
    }
    if memory(operand).is_some() {
        return format!("X86_LOAD_{size_suffix}({})", ea(operand));
    }
    String::new()
}

/// Returns an immediate as unsigned hex, cut to `bits`.
///
/// # Examples
///
/// ```text
/// constant(0xFFFF_FFFF_FFFF_FFFF, 8)   ->  "0xFFu"
/// constant(0xFFFF_FFFF_FFFF_FFFF, 32)  ->  "0xFFFFFFFFu"
/// constant(0x20, 32)                   ->  "0x20u"
/// ```
pub(super) fn constant(value: u64, bits: u16) -> String {
    let mask = u64::MAX >> (64 - bits);
    format!("0x{:X}u", value & mask)
}

/// Returns the C type for an operand size in bits.
///
/// # Examples
///
/// ```text
/// operand_type_name(8)   ->  "uint8_t"
/// operand_type_name(16)  ->  "uint16_t"
/// operand_type_name(32)  ->  "uint32_t"
/// ```
///
/// # Panics
///
/// Panics unless `size` is 8, 16 or 32.
#[inline]
pub(super) fn operand_type_name(size: u16) -> &'static str {
    match size {
        8 => "uint8_t",
        16 => "uint16_t",
        32 => "uint32_t",
        _ => panic!("Unsupported operand size"),
    }
}

/// Returns whether `operand` is memory that uses `segment`.
#[inline]
pub(super) fn segment_equals(operand: &DecodedOperand, segment: Register) -> bool {
    match &operand.kind {
        DecodedOperandKind::Mem(mem) => mem.segment == segment,
        _ => false,
    }
}

/// Reads `operand` as a register, even when it isn't one.
///
/// # Panics
///
/// Panics for pointer operands.
#[inline]
pub(super) fn operand_reg_value(operand: &DecodedOperand) -> Register {
    match &operand.kind {
        DecodedOperandKind::Reg(reg) => *reg,
        DecodedOperandKind::Mem(mem) => match mem.ty {
            MemoryOperandType::INVALID => Register::NONE,
            MemoryOperandType::MEM => Register::AL,
            MemoryOperandType::AGEN => Register::CL,
            MemoryOperandType::MIB => Register::DL,
            MemoryOperandType::VSIB => Register::BL,
        },
        DecodedOperandKind::Imm(imm) => {
            // The register overlays is_signed | (is_relative << 8).
            match (imm.is_signed, imm.is_relative) {
                (false, false) => Register::NONE,
                (true, false) => Register::AL,
                (false, true) => Register::BND0,
                (true, true) => Register::BND1,
            }
        }
        DecodedOperandKind::Ptr(_) => {
            panic!("reg.value read on a pointer operand")
        }
        DecodedOperandKind::Unused => Register::NONE,
    }
}

/// Writes `base + index * scale +/- displacement`, with numbers in hex or decimal.
///
/// # Examples
///
/// ```text
/// address_expression([esi+eax*4+8], true)   ->  "ctx.esi.u32 + ctx.eax.u32 * 0x4 + 0x8"
/// address_expression([esi+eax*4+8], false)  ->  "ctx.esi.u32 + ctx.eax.u32 * 4 + 8"
/// address_expression([], true)              ->  "0"
/// ```
fn address_expression(mem: &MemoryInfo, hex: bool) -> String {
    let number = |value: u32| {
        if hex {
            format!("0x{value:x}")
        } else {
            value.to_string()
        }
    };
    let mut result = String::new();

    if mem.base != Register::NONE {
        result.push_str(&get_addr_reg(mem.base));
    }

    if mem.index != Register::NONE {
        if !result.is_empty() {
            result.push_str(" + ");
        }
        result.push_str(&get_addr_reg(mem.index));
        if mem.scale > 1 {
            result.push_str(&format!(" * {}", number(u32::from(mem.scale))));
        }
    }

    let displacement = mem.disp.displacement;
    if displacement != 0 {
        if result.is_empty() {
            // Zydis sign-extends here.
            result = number(displacement as u32);
        } else {
            result.push_str(if displacement > 0 { " + " } else { " - " });
            result.push_str(&number(displacement.unsigned_abs() as u32));
        }
    }

    if result.is_empty() {
        result.push('0');
    }
    result
}
