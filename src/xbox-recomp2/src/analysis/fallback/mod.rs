//! Scan for functions the initial code walk may have missed.

use super::{graph::DiscoveryGraph, pointer_tables::PointerScan};
use crate::config::RecompilerConfig;
use crate::decode::Decoder;
use crate::executable::Executable;
#[cfg(test)]
use xbox_recomp2_executable::GuestAddress32;

mod code;
mod covered_starts;
mod padding;
mod refs;
mod tails;
#[cfg(test)]
mod test_helpers;

/// Find possible function starts from code references, padding, and tail jumps.
/// Read function-discovery.md and source for more info.
///
/// ## Code references
///
/// We scan every code section, including code the first walk never reached.
/// We look for calls, callbacks, and pointer tables:
///
/// ```text
/// 0x1000: E8 8C 00 00 00        call 0x1091            ; direct target
///      or 68 80 10 00 00        push 0x1080            ; callback
///      or B8 80 10 00 00        mov eax, 0x1080        ; callback
///      or FF 14 85 00 20 00 00  call [eax*4 + 0x2000]  ; table reference
/// 0x1080: 55 89 E5 C3           push ebp; mov ebp, esp; ret
/// 0x1091: C3                    ret
/// 0x2000: 91 10 00 00           dd 0x1091              ; table entry
/// ```
///
/// ## Function padding
///
/// We check every 16-byte boundary for possible function starts.
/// Each padding pattern ends with `CC` (`int3`) just before the boundary:
///
/// ```text
/// 0x107E: CC CC        int3; int3              ; padding
/// 0x1080: 55 89 E5 C3  push ebp; mov ebp, esp; ret ; aligned candidate
/// ```
///
/// ## Tail jumps
///
/// A tail call avoids returning to a function that has nothing left to do.
///
/// If the calling convention and stack layout allow it, the compiler can
/// use a jump instead:
///
/// ```asm
/// ; A has prepared B's arguments on the stack.
/// jmp B   ; B returns directly to A's caller
/// ```
///
/// We look for direct jumps whose targets lie outside the range between
/// nearby candidate starts. These targets may be separate functions.
///
/// ## Scan limits
///
/// We stay within section and file boundaries and skip banned or invalid ranges.
/// If decoding fails, we skip one byte and keep going.
///
/// Data can look like code, so some findings may not be functions.
/// Pointer tables can also change at runtime.
pub(super) fn collect(
    executable: &Executable,
    config: &RecompilerConfig,
    decoder: &Decoder,
    graph: &mut DiscoveryGraph,
) {
    // To validate our found pointers, e.g. `jmp rel32` (the rel32)
    let mut scan = PointerScan::new(executable, config, decoder);

    // Know which byte ranges we already covered.
    let covered = covered_starts::CoveredStarts::new(graph, config);

    refs::collect(executable, decoder, &mut scan, graph);
    padding::collect(executable, decoder, &mut scan, &covered, graph);
    tails::collect(executable, decoder, &mut scan, graph);
}
