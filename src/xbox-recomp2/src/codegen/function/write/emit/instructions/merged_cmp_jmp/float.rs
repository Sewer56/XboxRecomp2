//! Writes a merged float compare: `comiss`, `ucomiss`, `fcomip` or `fucomip`.

use super::super::prelude::*;
use super::CompareOutput;
use crate::codegen::function::model::{Condition, MergedCmpJmp, MergedInstructionKind};
use crate::decode::CpuFlag;

/// Writes a float compare's test for its jumps and selects.
///
/// # Returns
///
/// - [`CompareOutput::Float`] when every jump and select tests the same flags.
/// - [`CompareOutput::Flags`] when they test different flags.
/// - [`None`] if we can't write the SSE operands as C++.
///
/// # Examples
///
/// For these instructions:
///
/// - `comiss xmm0, [esi]`
/// - `mov [esi], ecx` <- may change `[esi]`
/// - `jbe 0x00401010`
///
/// We append:
///
/// ```text
/// const bool compare_taken = ctx.branch_ss<true, false, 0x41>(
///     _mm_cvtss_f32(ctx.xmm[0].reg), X86_LOAD_F32(ctx.esi.u32));
/// ```
///
/// With `jb` and `jz` instead, we append:
///
/// ```text
/// const uint32_t compare_flags = ctx.compare_flags_ss<true>(
///     _mm_cvtss_f32(ctx.xmm[0].reg), X86_LOAD_F32(ctx.esi.u32));
/// ```
pub(super) fn write_compare(ctx: &mut Emitter<'_>, merged: &MergedCmpJmp) -> Option<CompareOutput> {
    let tested = tested_flags(ctx.insn.mnemonic(), merged);
    if matches!(ctx.insn.mnemonic(), Mnemonic::FCOMIP | Mnemonic::FUCOMIP) {
        let pop = merged.x87_pop.is_some();
        return Some(super::super::x87::merged_fcomi(ctx, tested, pop));
    }

    let (lhs, rhs) = super::super::sse::compare_operands(ctx)?;
    let signalling = ctx.insn.mnemonic() == Mnemonic::COMISS;
    let status = !ctx.config.speed_hacks.runs_fast_sse_status();

    // Different flags: keep CF, ZF and PF.
    // With status flags, clang does as well with plain eflags as with a local.
    let Some(mask) = tested else {
        if status {
            cpp!(ctx.out, "ctx.compare_ss<{signalling}>({lhs}, {rhs});");
            return Some(CompareOutput::Flags("ctx.eflags.common".to_owned()));
        }
        cpp!(
            ctx.out,
            "const uint32_t compare_flags = ctx.compare_flags_ss<{signalling}>({lhs}, {rhs});"
        );
        return Some(CompareOutput::Flags("compare_flags".to_owned()));
    };

    // Same flags: one `bool`.
    let mask = mask.bits();
    let test = format!("ctx.branch_ss<{signalling}, {status}, 0x{mask:X}>({lhs}, {rhs})");
    if merged.instructions.len() == 1 {
        return Some(CompareOutput::Float(test));
    }
    cpp!(ctx.out, "const bool compare_taken = {test};");
    Some(CompareOutput::Float("compare_taken".to_owned()))
}

/// The flags every jump and select tests.
///
/// # Returns
///
/// [`None`] when they test different flags.
fn tested_flags(compare: Mnemonic, merged: &MergedCmpJmp) -> Option<CpuFlag> {
    let mut tested = None;
    for insn in &merged.instructions {
        if insn.kind == MergedInstructionKind::Plain {
            continue;
        }

        let flags = match MergedCmpJmp::condition(compare, insn.guest.insn.mnemonic()) {
            Some(Condition::Flags(flags, _)) => flags,
            _ => unreachable!("jumps and selects after float compares test flags"),
        };
        if *tested.get_or_insert(flags) != flags {
            return None;
        }
    }
    Some(tested.expect("merged ops end with a jump or select"))
}
