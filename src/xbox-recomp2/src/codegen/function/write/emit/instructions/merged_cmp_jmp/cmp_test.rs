//! Reads the values a merged `cmp` or `test` compares.

use super::super::prelude::*;
use super::{IntegerCompare, written_later};
use crate::codegen::function::model::{MergedCmpJmp, MergedInstructionKind};
use crate::decode::CpuFlag;

/// Reads a `cmp` or `test`'s operands.
///
/// Any that change before jump/select are copied.
///
/// # Returns
///
/// The compare's operands and result, for the jumps and selects.
/// [`None`] if we can't write an operand as C++.
///
/// # Examples
///
/// `cmp eax, [esi]`, with a `mov [esi], ecx` before the jump:
///
/// ```text
/// const uint32_t rhs = X86_LOAD_U32(ctx.esi.u32);
/// ```
///
/// ```text
/// cmp eax, ebx    ; result: (ctx.eax.u32 - ctx.ebx.u32)
/// ```
///
/// ```text
/// test eax, ebx   ; result: (ctx.eax.u32 & ctx.ebx.u32)
/// ```
///
/// ```text
/// test eax, eax   ; result: ctx.eax.u32
/// ```
pub(super) fn write_compare(
    ctx: &mut Emitter<'_>,
    merged: &MergedCmpJmp,
) -> Option<IntegerCompare> {
    let mnemonic = ctx.insn.mnemonic();
    let (mut lhs, mut rhs) = super::super::compare::operands(ctx)?;
    let operands = ctx.insn.operands();
    let bits = operands[0].size;

    // `cmp al, -1` must test `0xFFu`, not `0xFFFFFFFFu`. (width fix for underflow)
    for (value, operand) in [(&mut lhs, &operands[0]), (&mut rhs, &operands[1])] {
        if let Some(imm) = immediate(operand) {
            *value = constant(imm.value, bits);
        }
    }

    // `test eax, eax` reads `eax` once, as `eax == 0`.
    let test = mnemonic == Mnemonic::TEST;
    let same = test && lhs == rhs;

    // const uint32_t rhs = X86_LOAD_U32(ctx.esi.u32);
    if needs_copy(merged, &operands[0]) {
        let name = if same { "value" } else { "lhs" };
        cpp!(ctx.out, "const uint{bits}_t {name} = {lhs};");
        lhs = name.to_owned();
    }

    if same {
        rhs.clone_from(&lhs);
    } else if needs_copy(merged, &operands[1]) {
        cpp!(ctx.out, "const uint{bits}_t rhs = {rhs};");
        rhs = "rhs".to_owned();
    }

    // (ctx.eax.u32 - ctx.ebx.u32)
    // (ctx.eax.u32 & ctx.ecx.u32)
    let result = if !test {
        expression(&format!("{lhs} - {rhs}"), bits)
    } else if same {
        lhs.clone()
    } else {
        expression(&format!("{lhs} & {rhs}"), bits)
    };
    Some(IntegerCompare {
        mnemonic,
        lhs,
        rhs,
        result,
        bits,
    })
}

/// Whether we must copy `operand` before the merged instructions run.
///
/// - Registers: if a merged instruction may write them.
/// - Memory: unless one jump right after the compare is its only use.
///
/// # Examples
///
/// ```text
/// cmp eax, ebx
/// mov eax, 0      ; writes eax: copy eax
/// jz  equal
/// ```
///
/// ```text
/// cmp eax, [esi]
/// jz  equal       ; only use: no copy
/// ```
///
/// ```text
/// cmp eax, [esi]
/// mov [esi], ecx  ; may change [esi]: copy [esi]
/// jz  equal
/// ```
///
/// ```text
/// cmp eax, [esi]
/// jz  equal
/// jb  below       ; second use: copy [esi], so we load it once
/// ```
fn needs_copy(merged: &MergedCmpJmp, operand: &DecodedOperand) -> bool {
    if register(operand).is_some_and(|reg| written_later(merged, reg)) {
        return true;
    }

    memory(operand).is_some() && !used_once(merged)
}

fn used_once(merged: &MergedCmpJmp) -> bool {
    let plain_jump = MergedInstructionKind::Jump(CpuFlag::empty());
    merged.end_flags.is_empty()
        && matches!(merged.instructions.as_slice(), [insn] if insn.kind == plain_jump)
}

/// `value` as a `bits`-wide value that's safe to use in a larger expression.
///
/// C++ widens 8- and 16-bit math to `int`, so those get cut back.
/// 32-bit math gets parentheses: `a & b == 0` means `a & (b == 0)`.
///
/// # Examples
///
/// ```text
/// (ctx.eax.u32 & ctx.ecx.u32)
/// static_cast<uint8_t>(ctx.eax.u8_lo & 0x1u)
/// ```
fn expression(value: &str, bits: u16) -> String {
    if bits == 32 {
        format!("({value})")
    } else {
        format!("static_cast<uint{bits}_t>({value})")
    }
}
