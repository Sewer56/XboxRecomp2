//! Writes the C++ for a [`MergedCmpJmp`].
//!
//! - [`write_compare`] writes the compare.
//! - [`write_jump_or_select`] writes one merged jump, SETcc or CMOVcc.
//! - [`write_end_flags`] writes the flags that later code still reads.

mod arithmetic;
mod cmp_test;
mod condition;
mod float;

use super::prelude::*;
use crate::codegen::function::model::{Condition, MergedCmpJmp, MergedInstructionKind};
use crate::codegen::optimizations::helpers::instruction::register_access::RegisterAccess;
use crate::decode::CpuFlag;

/// What [`write_compare`] leaves for the jumps and selects to test.
pub(in crate::codegen::function::write) enum CompareOutput {
    /// An integer compare's operands and result.
    Integer(IntegerCompare),

    /// A float compare's test, as C++.
    ///
    /// ```text
    /// comiss xmm0, xmm1   ; Float("ctx.branch_ss<true, false, 0x1>(...)")
    /// jb     below        ; tests that directly
    /// ```
    ///
    /// ```text
    /// comiss xmm0, xmm1   ; Float("compare_taken")
    /// jb     below        ; compare_taken
    /// jnb    above        ; !compare_taken
    /// ```
    ///
    /// ```text
    /// fcomip st0, st1     ; Float("fcomi_taken")
    /// jb     below        ; fcomi_taken
    /// ```
    Float(String),

    /// Where the jumps and selects read the flags, as C++.
    ///
    /// ```text
    /// sahf                ; Flags("ctx.eax.u8_hi")
    /// jz  equal           ; (ctx.eax.u8_hi & X86EFlags::ZF) != 0
    /// ```
    ///
    /// ```text
    /// sahf                ; Flags("flags"), a copy of ah
    /// mov ah, 0           ; changes ah
    /// jz  equal           ; (flags & X86EFlags::ZF) != 0
    /// ```
    ///
    /// ```text
    /// comiss xmm0, xmm1   ; Flags("compare_flags")
    /// jb     below        ; (compare_flags & X86EFlags::CF) != 0
    /// jz     equal        ; (compare_flags & X86EFlags::ZF) != 0
    /// ```
    Flags(String),
}

/// An integer compare's operands and result, as C++.
///
/// # Examples
///
/// ```text
/// cmp eax, ebx
/// jb  below       ; {lhs} < {rhs}
/// ```
///
/// ```text
/// cmp eax, ebx
/// jl  less        ; static_cast<int{bits}_t>({lhs}) < static_cast<int{bits}_t>({rhs})
/// ```
///
/// ```text
/// dec ecx
/// jz  zero        ; {result} == 0
/// ```
///
/// ```text
/// test eax, eax
/// js   negative   ; static_cast<int{bits}_t>({result}) < 0
/// ```
///
/// ```text
/// test al, al
/// jp   even       ; __builtin_parity({result}) == 0
/// ```
///
/// ```text
/// add eax, ebx
/// jb  wrapped     ; {result} < {lhs}
/// ```
///
/// ```text
/// sub eax, ebx
/// jz  zero        ; {result} == 0
/// jb  below       ; {lhs} < {rhs}
/// ```
pub(in crate::codegen::function::write) struct IntegerCompare {
    /// The compare, like `cmp` or `dec`.
    mnemonic: Mnemonic,

    /// The first operand, like `ctx.eax.u32`.
    lhs: String,

    /// The second operand, like `0x20u` or `ctx.ebx.u32`.
    rhs: String,

    /// The result, like `ctx.eax.u32` or `result`.
    ///
    /// - `cmp eax, ebx`: `(ctx.eax.u32 - ctx.ebx.u32)`
    /// - `test eax, ebx`: `(ctx.eax.u32 & ctx.ebx.u32)`
    /// - `test eax, eax`: `ctx.eax.u32`
    result: String,

    /// The width: 8, 16 or 32.
    bits: u16,
}

/// Writes the compare.
///
/// # Returns
///
/// What the jumps and selects test.
/// [`None`] if we can't write an operand as C++.
///
/// # Examples
///
/// For these instructions, this appends:
///
/// - `cmp eax, [esi]`
/// - `mov [esi], ecx` <- may change `[esi]`
/// - `jz 0x00401010`
///
/// ```text
/// const uint32_t rhs = X86_LOAD_U32(ctx.esi.u32);
/// ```
///
/// For `dec ecx` then `jnz 0x00401010`:
///
/// ```text
/// ctx.ecx.u32 -= 1;
/// ```
pub(in crate::codegen::function::write) fn write_compare(
    ctx: &mut Emitter<'_>,
    merged: &MergedCmpJmp,
) -> Option<CompareOutput> {
    match ctx.insn.mnemonic() {
        Mnemonic::SAHF => Some(CompareOutput::Flags(sahf_flags(ctx, merged))),
        Mnemonic::COMISS | Mnemonic::UCOMISS | Mnemonic::FCOMIP | Mnemonic::FUCOMIP => {
            float::write_compare(ctx, merged)
        }
        Mnemonic::CMP | Mnemonic::TEST => {
            cmp_test::write_compare(ctx, merged).map(CompareOutput::Integer)
        }
        _ => Some(CompareOutput::Integer(arithmetic::write_compare(
            ctx, merged,
        ))),
    }
}

/// Writes one jump, SETcc or CMOVcc.
///
/// # Examples
///
/// For the `jz` in these instructions, this appends:
///
/// from
/// - `cmp eax, ebx`
/// - `jz 0x00401010`
///
/// to
/// ```text
/// if (ctx.eax.u32 == ctx.ebx.u32) goto loc_401010;
/// ```
///
/// For a `jnz` that needs CF when taken:
///
/// ```text
/// if (ctx.eax.u32 != ctx.ebx.u32) {
///     ctx.eflags.update_compare_flags_U32<0x1>(ctx.eax.u32, ctx.ebx.u32);
///     goto loc_401010;
/// }
/// ```
pub(in crate::codegen::function::write) fn write_jump_or_select(
    ctx: &mut Emitter<'_>,
    compare: Mnemonic,
    output: &CompareOutput,
    kind: MergedInstructionKind,
) {
    let condition = MergedCmpJmp::condition(compare, ctx.insn.mnemonic())
        .expect("merging validates the condition");

    let test = output.test(condition);
    match kind {
        MergedInstructionKind::Jump(flags) => {
            // if (ctx.eax.u32 == ctx.ebx.u32) goto loc_401010;
            let write = (!flags.is_empty()).then(|| output.integer().flag_write(flags));
            super::control_transfer::helpers::print_conditional_branch_with(
                ctx,
                false,
                &test,
                write.as_deref(),
            );
        }
        MergedInstructionKind::Select => {
            // ctx.edx.u32 = (ctx.edi.u32 > ctx.ebx.u32) ? ctx.edi.u32 : ctx.edx.u32;
            super::data_transfer::write_conditional(ctx, &test)
        }
        MergedInstructionKind::Plain => unreachable!("plain instructions are written as usual"),
    }
}

/// Writes the flags that code after the merge still reads.
///
/// # Examples
///
/// For `cmp eax, ebx` with ZF read later, this appends:
///
/// ```text
/// ctx.eflags.update_compare_flags_U32<0x40>(ctx.eax.u32, ctx.ebx.u32);
/// ```
pub(in crate::codegen::function::write) fn write_end_flags(
    ctx: &mut Emitter<'_>,
    output: &CompareOutput,
    flags: CpuFlag,
) {
    if !flags.is_empty() {
        let write = output.integer().flag_write(flags);
        cpp!(ctx.out, "{write}");
    }
}

impl CompareOutput {
    /// The C++ test for `condition`, like `ctx.eax.u32 == 0x20u`.
    fn test(&self, condition: Condition) -> String {
        match self {
            CompareOutput::Integer(compare) => compare.test(condition),
            CompareOutput::Float(taken) => condition::float_test(taken, condition),
            CompareOutput::Flags(flags) => condition::flags_test(flags, condition),
        }
    }

    /// The integer compare, for writing flags.
    fn integer(&self) -> &IntegerCompare {
        match self {
            CompareOutput::Integer(compare) => compare,
            _ => unreachable!("float compares and sahf merge only when no flags are needed"),
        }
    }
}

/// Reads the flags `sahf` loads.
///
/// # Examples
///
/// ```text
/// const uint8_t flags = ctx.eax.u8_hi;
/// ```
///
/// Flags are copied if `ah` changes before the last jump or select.
fn sahf_flags(ctx: &mut Emitter<'_>, merged: &MergedCmpJmp) -> String {
    if !written_later(merged, Register::AH) {
        return "ctx.eax.u8_hi".to_owned();
    }
    cpp!(ctx.out, "const uint8_t flags = ctx.eax.u8_hi;");
    "flags".to_owned()
}

/// Whether an instruction merged after the compare may write `reg`.
fn written_later(merged: &MergedCmpJmp, reg: Register) -> bool {
    merged
        .instructions
        .iter()
        .any(|insn| RegisterAccess::of(&insn.guest.insn).writes(reg))
}
