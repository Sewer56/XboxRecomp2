//! Writes `<name>` for a function whose calling convention was changed in order
//! for code to go brrrrrr.
//!
//! Basically, optimised calls we rewrote go straight to `<name>_fastcall`.
//!
//! Everything else, like:
//! - Pointer/Indirect calls
//! - Host-side hooks
//!
//! Call the original `<name>`.
//!
//! e.g.
//! ```cpp
//! X86_PROTECTED_FUNC(sub_1040) {
//!     ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4); // argument 0, passed in eax
//!     ctx.esp.u32 += 0x4;                            // drop the return address
//!     sub_1040_fastcall(ctx, base);
//! }
//! ```

use super::emit::{get_x86_reg, xmm_index};
use crate::codegen::function::model::FastcallConvention;
use crate::codegen::optimizations::helpers::stack_frame::STACK_WORD_BYTES;
use crate::decode::RegisterClass;
use crate::templates::FASTCALL_SUFFIX;
use core::fmt::Write as _;

/// Appends the original entry, `name`, to `text`.
pub(super) fn write(fastcall: &FastcallConvention, name: &str, text: &mut String) {
    let _ = writeln!(text, "X86_PROTECTED_FUNC({name}) {{");

    // Load the register arguments first.
    for &(argument, register) in &fastcall.register_arguments {
        let offset = STACK_WORD_BYTES * (u32::from(argument) + 1); // +1 for return address
        let address = format!("ctx.esp.u32 + 0x{offset:X}");
        let _ = if register.class() == RegisterClass::XMM {
            // Like `movss xmm, [mem]`
            let index = xmm_index(register);
            writeln!(
                text,
                "\tctx.xmm[{index}].reg = _mm_set_ss(X86_LOAD_F32({address}));"
            )
        } else {
            let destination = get_x86_reg(register);
            writeln!(text, "\t{destination} = X86_LOAD_U32({address});")
        };
    }

    if fastcall.omits_return_slot {
        let _ = writeln!(text, "\tctx.esp.u32 += 0x{STACK_WORD_BYTES:X};");
    }
    let _ = writeln!(text, "\t{name}{FASTCALL_SUFFIX}(ctx, base);\n}}\n");
}
