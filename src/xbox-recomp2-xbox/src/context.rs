//! Xbox specific stuff added to `x86_context.h`.
//!
//! Basically the code to splice together [`x86_context_xbox.h`]
//! and the original `x86_context.h` from the other crate for the final
//! output file.
//!
//! [`x86_context_xbox.h`]: self

use xbox_recomp2::assemble_context_header;
use xbox_recomp2::platform::ContextOverlay;

/// Splice Xbox specific header data with generic x86 context.
///
/// # Returns
///
/// The final `x86_context.h`.
pub fn context_header() -> Vec<u8> {
    assemble_context_header(&overlay())
}

/// Xbox specific additions to common header.
pub(crate) fn overlay() -> ContextOverlay {
    split_overlay(include_str!("x86_context_xbox.h"))
}

/// Split common overlay:
/// - `//@state` for the structs' members
/// - `//@declarations` for... declarations.
fn split_overlay(source: &'static str) -> ContextOverlay {
    let (_, after_state) = source
        .split_once("\n//@state\n")
        .expect("the overlay file carries a //@state marker line");
    let (state, declarations) = after_state
        .split_once("\n//@declarations\n")
        .expect("the overlay file carries a //@declarations marker line");

    ContextOverlay {
        // The file's trailing newline is not part of the snippet.
        declarations: declarations.strip_suffix('\n').unwrap_or(declarations),
        extra_state: state,
    }
}
