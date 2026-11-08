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

use super::prelude::Emitter;

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
