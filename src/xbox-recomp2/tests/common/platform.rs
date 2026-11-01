//! A platform that never asks for special handling, so tests see plain output.

use xbox_recomp2::executable::ImportMap;
use xbox_recomp2::platform::{GuestPlatform, MemoryRoute};
use zydis::{Mnemonic, Register};

/// The platform every core integration test passes to the recompiler.
pub(crate) static PLATFORM: PlainPlatform = PlainPlatform;

/// Treats all memory as normal RAM and never redirects import calls.
///
/// Basically does none of the Xbox specific stuff.
pub(crate) struct PlainPlatform;

impl GuestPlatform for PlainPlatform {
    fn route(
        &self,
        _imports: &ImportMap,
        _insn: Mnemonic,
        _segment: Register,
        _address: u32,
        _size: u16,
        _is_write: bool,
    ) -> MemoryRoute {
        MemoryRoute::Plain
    }

    fn register_call_window(&self) -> Option<(u32, u32)> {
        None
    }
}
