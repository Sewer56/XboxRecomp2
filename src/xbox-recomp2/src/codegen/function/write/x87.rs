//! Writes the x87 depth hints and the local `fpu` copy's setup.
//!
//! The `optimize_x87` pass decides where they go; see `x87.md`.

use super::emit::FunctionState;
use crate::codegen::function::model::{X87Level, X87Plan};
use core::fmt::Write as _;
use xbox_recomp2_executable::GuestAddress32;

/// Picks up the syncs for the instruction at `address`.
///
/// Replaces `text` with its depth hint, if it has one, like:
///
/// ```text
/// X87_ASSUME_DEPTH(fpu, 1);
/// ```
pub(super) fn start_instruction(
    x87: &X87Plan,
    address: GuestAddress32,
    function_state: &mut FunctionState,
    text: &mut String,
) {
    text.clear();
    function_state.x87_sync = x87.instructions.get(&address).copied().unwrap_or_default();
    if let Some(depth) = function_state.x87_sync.assume {
        let fpu = function_state.fpu();
        let _ = writeln!(text, "\tX87_ASSUME_DEPTH({fpu}, {depth});");
    }
}

/// Writes the local x87 copy, and the entry depth hint.
///
/// # Examples
///
/// With the local copy:
///
/// ```text
/// X87FPU fpu = x87_copy(ctx.fpu);
/// X87_ASSUME_DEPTH(fpu, 0);
/// ```
pub(super) fn write_prologue(x87: &X87Plan, text: &mut String) {
    let entry = x87.entry;
    match x87.level {
        X87Level::Original => {}
        X87Level::Hints => {
            let _ = writeln!(text, "\tX87_ASSUME_DEPTH(ctx.fpu, {entry});");
        }
        X87Level::LocalCopy => {
            let _ = writeln!(
                text,
                "\tX87FPU fpu = x87_copy(ctx.fpu);\n\tX87_ASSUME_DEPTH(fpu, {entry});"
            );
        }
    }
}
