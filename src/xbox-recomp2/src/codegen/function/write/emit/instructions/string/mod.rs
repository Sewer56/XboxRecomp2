//! String instructions like `movsb` and `stosd`, plus `xlat`.

pub(super) mod cmpsb;
pub(super) mod cmpsd;
pub(super) mod cmpsw;
pub(super) mod movsb;
pub(super) mod movsd;
pub(super) mod movsw;
pub(super) mod scasb;
pub(super) mod stosb;
pub(super) mod stosd;
pub(super) mod stosw;
pub(super) mod xlat;

use super::prelude::{Emitter, cpp};
use crate::codegen::function::model::DirectionFlag;

/// Returns the C++ condition for "this string instruction runs backward".
/// As:
/// - True
/// - False
/// - Unknown (Check at runtime)
fn backward(ctx: &Emitter<'_>) -> &'static str {
    match ctx.direction {
        DirectionFlag::Unknown => "ctx.eflags.rare & X86EFlags::DF",
        DirectionFlag::Forward => "false",
        DirectionFlag::Backward => "true",
    }
}

/// Before a REP MOVS loop, writes an `if` that tries `memmove` first.
///
/// # Arguments
///
/// - `element`: the C++ element type, like `uint32_t` for `movsd`.
fn try_memmove(ctx: &mut Emitter<'_>, element: &str) {
    if uses_memmove_memset(ctx) {
        let backward = backward(ctx);
        cpp!(
            ctx.out,
            "if (!x86_rep_movs_memmove<{element}>(base, ctx.esi.u32, ctx.edi.u32, ctx.ecx.u32, {backward}))"
        );
    }
}

/// Before a REP STOS loop, writes an `if` that tries `memset` first.
///
/// # Arguments
///
/// - `value`: the C++ fill value, like `ctx.eax.u32` for `stosd`.
fn try_memset(ctx: &mut Emitter<'_>, value: &str) {
    if uses_memmove_memset(ctx) {
        let backward = backward(ctx);
        cpp!(
            ctx.out,
            "if (!x86_rep_stos_memset(base, ctx.edi.u32, ctx.ecx.u32, {value}, {backward}))"
        );
    }
}

/// Whether a REP MOVS or REP STOS may try `memmove` or `memset` first.
fn uses_memmove_memset(ctx: &Emitter<'_>) -> bool {
    ctx.config.speed_hacks.runs_rep_memmove_memset()
        && ctx.native_addresses()
        && ctx.insn.instruction().address_width == 32
}

/// Returns the C++ types for a REP loop's address.
fn rep_types(ctx: &Emitter<'_>) -> (&'static str, &'static str) {
    if ctx.native_addresses() {
        ("uintptr_t", "intptr_t")
    } else {
        ("uint32_t", "uint32_t")
    }
}

/// Returns the first REP STOS address.
fn rep_start(ctx: &Emitter<'_>) -> &'static str {
    if ctx.native_addresses() {
        // As a `uintptr_t`, adding the memory base doesn't wrap at 4 GB.
        "static_cast<uintptr_t>(ctx.edi.u32)"
    } else {
        "ctx.edi.u32"
    }
}
