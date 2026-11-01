//! Builds the body of the generated `x86_context.h`.
//!
//! The shared x86 parts live in `x86_context_core.h`.
//! Platforms add their own parts via [`ContextOverlay`].
//!
//! Those go where the core has `//@context:` marker lines.
//! So does `x86_portable_simd.h`, for non-x86 hosts.

use crate::platform::ContextOverlay;

/// The shared x86 header body.
pub(crate) const CONTEXT_HEADER_CORE: &[u8] = include_bytes!("x86_context_core.h");
// Lines replaced by the platform overlay.
const DECLARATIONS_MARKER: &[u8] = b"//@context:declarations";
const EXTRA_STATE_MARKER: &[u8] = b"//@context:extra-state";
/// The intrinsics the generated code uses, for hosts without SSE.
const PORTABLE_SIMD: &str = include_str!("x86_portable_simd.h");
const PORTABLE_SIMD_MARKER: &[u8] = b"//@context:portable-simd";

/// Builds `x86_context.h` with platform-specific (e.g. Xbox) additions.
///
/// # Arguments
///
/// * `overlay` - platform specific context.
///
/// # Returns
///
/// Assembled header.
pub fn assemble_context_header(overlay: &ContextOverlay) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        CONTEXT_HEADER_CORE.len()
            + PORTABLE_SIMD.len()
            + overlay.declarations.len()
            + overlay.extra_state.len(),
    );

    for line in CONTEXT_HEADER_CORE.split_inclusive(|&byte| byte == b'\n') {
        // Replace the marker in place where it applies.
        let snippet = match line.strip_suffix(b"\n").unwrap_or(line) {
            DECLARATIONS_MARKER => overlay.declarations,
            EXTRA_STATE_MARKER => overlay.extra_state,
            PORTABLE_SIMD_MARKER => PORTABLE_SIMD.strip_suffix('\n').unwrap_or(PORTABLE_SIMD),
            _ => {
                out.extend_from_slice(line);
                continue;
            }
        };

        // Empty drops the line entirely.
        if !snippet.is_empty() {
            out.extend_from_slice(snippet.as_bytes());
            out.push(b'\n');
        }
    }
    out
}
