//! The Xbox [`GuestPlatform`]. Handles addresses that are special on Xbox.
//!
//! - `fs:[...]` is the kernel's per-CPU data. See [`fs`].
//! - Some `ds:[...]` addresses are hardware or kernel data. See [`ds`].
//! - `call eax` can call kernel functions. See [`REGISTER_CALL_WINDOW`].

use crate::context;
use xbox_recomp2::platform::{ContextOverlay, GuestPlatform, MemoryRoute};
use xbox_recomp2_executable::ImportMap;
use zydis::{Mnemonic, Register};

mod ds;
mod fs;
mod windows;

/// Register calls to this range are kernel thunk (import table) functions.
const REGISTER_CALL_WINDOW: (u32, u32) = (0x8000_0001, 0x8000_0360);
/// The Xbox platform. Pass this to the recompiler.
pub static XBOX: Xbox = Xbox;

/// The Xbox platform.
pub struct Xbox;

impl GuestPlatform for Xbox {
    fn route(
        &self,
        imports: &ImportMap,
        insn: Mnemonic,
        segment: Register,
        address: u32,
        size: u16,
        is_write: bool,
    ) -> MemoryRoute {
        match segment {
            Register::FS => fs::route(insn, address, size, is_write),
            Register::DS => ds::route(imports, insn, address, size, is_write),
            _ => MemoryRoute::Plain,
        }
    }

    fn register_call_window(&self) -> Option<(u32, u32)> {
        Some(REGISTER_CALL_WINDOW)
    }

    fn context_overlay(&self) -> ContextOverlay {
        context::overlay()
    }
}
