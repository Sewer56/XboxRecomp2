//! Everything that differs between target platforms, like Xbox.
//!
//! Some addresses mean something special on a console.
//! On Xbox:
//!
//! - `fs:[...]` points at the kernel's per-CPU data, not normal RAM.
//! - Some fixed addresses are hardware, like the GPU's registers.
//!
//! For these, we ask [`GuestPlatform::route`] what to write.
//! For example, the Xbox platform turns `push fs:[0x20]` into:
//!
//! ```cpp
//! X86_PUSH(ctx, base, X86_LOAD_U32(ctx.kpcr_ptr + 0x20));
//! ```

use crate::executable::ImportMap;
use zydis::{Mnemonic, Register};

/// Extra C a platform adds to the generated `x86_context.h`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextOverlay {
    /// Globals.
    /// Xbox adds:
    ///
    /// ```c
    /// extern uint8_t g_mmio[];
    /// ```
    pub declarations: &'static str,

    /// Extra fields for `struct X86Context`.
    ///
    /// Xbox adds:
    ///
    /// ```c
    /// uint32_t kpcr_ptr;
    /// ```
    pub extra_state: &'static str,
}

/// What to write for one memory access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryRoute {
    /// Write the access the normal way.
    Plain,

    /// Use this expression in place of the normal access.
    ///
    /// `mov`, `and`, `or` and `xor` replace one `{reg}` with the
    /// operand's register. `test` and `movzx` don't, so give them a
    /// complete expression.
    Expr(String),

    /// Write these lines instead of the normal code.
    ///
    /// One `{reg}` is replaced with the value's register. Each line
    /// becomes its own statement.
    Statement(String),
}

/// Everything specific to one target platform, like Xbox.
pub trait GuestPlatform {
    /// Says what to write when `insn` accesses `address` in `segment`.
    ///
    /// `size` is in bits. `imports` has the import addresses the loader
    /// found.
    fn route(
        &self,
        imports: &ImportMap,
        insn: Mnemonic,
        segment: Register,
        address: u32,
        size: u16,
        is_write: bool,
    ) -> MemoryRoute;

    /// Returns the address range, `(low, high)` inclusive, for
    /// indirect calls to imports.
    ///
    /// If a register call hits a loaded import address in this range,
    /// it jumps back to that import's thunk. Return [`None`] to turn
    /// this off.
    fn register_call_window(&self) -> Option<(u32, u32)>;

    /// Returns the extra C this platform adds to `x86_context.h`.
    ///
    /// Adds nothing by default.
    fn context_overlay(&self) -> ContextOverlay {
        ContextOverlay::default()
    }
}

/// Returns the macro suffix for an access size, as in `X86_LOAD_U32`.
///
/// # Arguments
///
/// * `size` - access size in bits.
///
/// # Returns
///
/// `U8`, `U16` or `U32`.
///
/// # Panics
///
/// Panics unless `size` is 8, 16 or 32.
#[inline]
pub fn operand_size_name(size: u16) -> &'static str {
    match size {
        8 => "U8",
        16 => "U16",
        32 => "U32",
        _ => panic!("Unsupported operand size"),
    }
}
