//! How the writer lowers one function's x87 code.
//!
//! Filled in by the `optimize_x87` pass. See `x87.md`.

use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// How to write one function's x87 code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct X87Plan {
    /// How to write the whole function.
    pub level: X87Level,

    /// Values on the x87 stack when the function starts.
    ///
    /// Written as `X87_ASSUME_DEPTH(fpu, 1)` at the top.
    pub entry: u8,

    /// Extra code for single instructions, by guest address.
    pub instructions: BTreeMap<GuestAddress32, X87Sync>,
}

/// How to write a whole function's x87 code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum X87Level {
    /// Left as it is.
    #[default]
    Original,

    /// x87 code runs on `ctx.fpu`.
    ///
    /// Starts with `X87_ASSUME_DEPTH(ctx.fpu, N)`, `N` from [`X87Plan::entry`].
    Hints,

    /// x87 code runs on a local `X87FPU fpu`, synced with `ctx.fpu` at calls,
    /// returns and hooks.
    ///
    /// Starts with `X87FPU fpu = x87_copy(ctx.fpu);` and
    /// `X87_ASSUME_DEPTH(fpu, N)`, `N` from [`X87Plan::entry`].
    LocalCopy,
}

/// Extra code around one instruction.
///
/// Each number is how many values are on the x87 stack at that point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct X87Sync {
    // ── Before the instruction ──────────────────────────────────────────────
    /// Tell clang the stack holds this many values.
    ///
    /// Written as `X87_ASSUME_DEPTH(fpu, 2)`.
    pub assume: Option<u8>,

    // ── Instead of the instruction ──────────────────────────────────────────
    /// Write this `ffree` as a pop. (The `fast_x87_ffree` speed hack.)
    pub ffree_pops: bool,

    /// Copy the local copy, holding this many values, back to `ctx.fpu`.
    ///
    /// Written as `x87_out<N>(ctx.fpu, fpu)`, which copies only those `N` slots.
    pub local_to_ctx: Option<u8>,

    // ── After the instruction ───────────────────────────────────────────────
    /// The call returned with this many values on the stack.
    /// Copy it back to local `fpu` from `ctx.fpu`.
    ///
    /// Written as `x87_in<N>(fpu, ctx.fpu)`.
    pub ctx_to_local: Option<u8>,

    // ── Around the instruction's mid-assembly hook ──────────────────────────
    /// Copy the local copy to `ctx.fpu` for the hook, and reload it after.
    pub hook: Option<HookSync>,
}

/// How to sync the local copy around a mid-assembly hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HookSync {
    /// Amount of items on x87 stack.
    pub depth: u8,

    /// Whether the local copy may hold changes `ctx.fpu` lacks
    /// (i.e. copy is needed).
    pub local_to_ctx: bool,
}
