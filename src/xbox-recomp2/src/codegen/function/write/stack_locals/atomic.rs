//! Rewrites `xchg`, `cmpxchg` and `lock xadd` that use a stack slot.
//!
//! We copy the slot to a local, run the register form on it, then store it back.
//!
//! Before:
//!
//! ```cpp
//! X86_CMPXCHG32_MEM<0x40>(ctx.esp.u32 + 0x4, ctx.ecx.u32, ctx, base);
//! ```
//!
//! After:
//!
//! ```cpp
//! std::atomic_thread_fence(std::memory_order_seq_cst);
//! {
//!     uint32_t slot = X86_FRAME_LOAD(uint32_t, 0xC);
//!     X86_CMPXCHG32<0x40>(slot, ctx.ecx.u32, ctx);
//!     X86_FRAME_STORE(uint32_t, 0xC, slot);
//! }
//! std::atomic_thread_fence(std::memory_order_seq_cst);
//! ```

use super::{FrameAccess, Insn, InstructionAttributes, Mnemonic, ea};
use crate::codegen::function::write::emit::get_x86_reg;
use crate::decode::register;

/// Writes `code` to `out` as an operation on a copy of its stack slot.
///
/// # Returns
///
/// `false` if `insn` is not one of these instructions, leaving `out` unchanged.
pub(super) fn rewrite(
    access: &FrameAccess,
    insn: &Insn,
    native: bool,
    code: &str,
    out: &mut String,
) -> bool {
    let &(index, offset) = match access.operands.first() {
        Some(slot) => slot,
        None => return false,
    };
    let operand = &insn.operands()[usize::from(index)];
    let bits = operand.size;

    // Run the register form on `slot`.
    let body = match insn.mnemonic() {
        Mnemonic::XCHG => {
            let other = &insn.operands()[usize::from(index == 0)];
            let reg = get_x86_reg(register(other).expect("xchg has a register operand"));
            format!("X86_XCHG_REG(slot, {reg});")
        }
        Mnemonic::CMPXCHG => {
            let helper = format!("X86_CMPXCHG{bits}");
            use_slot(code, &helper, "_MEM", &ea(operand, native))
        }
        Mnemonic::XADD if is_locked(insn) => {
            let helper = format!("X86_XADD{bits}");
            use_slot(code, &helper, "_ATOMIC", &ea(operand, native))
        }
        _ => return false,
    };

    // Load the slot, run it, store it back.
    let ty = format!("uint{bits}_t");
    *out = format!(
        "\t{{\n\
         \t\t{ty} slot = X86_FRAME_LOAD({ty}, 0x{offset:X});\n\
         \t\t{body}\n\
         \t\tX86_FRAME_STORE({ty}, 0x{offset:X}, slot);\n\
         \t}}\n"
    );
    fence(out);
    true
}

/// Puts a full memory barrier before and after `text`.
pub(super) fn fence(text: &mut String) {
    const FENCE: &str = "\tstd::atomic_thread_fence(std::memory_order_seq_cst);\n";
    text.insert_str(0, FENCE);
    text.push_str(FENCE);
}

fn is_locked(insn: &Insn) -> bool {
    insn.instruction()
        .attributes
        .contains(InstructionAttributes::HAS_LOCK)
}

/// Turns the memory form of `helper` in `code`
/// into its register form on `slot`.
///
/// ```text
/// Before: X86_XADD32_ATOMIC<0x8D5>(ctx.esp.u32 + 0x4, ctx.ecx.u32, ctx, base);
/// After:  X86_XADD32<0x8D5>(slot, ctx.ecx.u32, ctx);
/// ```
fn use_slot(code: &str, helper: &str, suffix: &str, address: &str) -> String {
    let body = code
        .replace(&format!("{helper}{suffix}"), helper)
        .replace(&format!("({address}, "), "(slot, ")
        .replace(", base)", ")");
    assert!(
        body.contains(helper) && body.contains("(slot, "),
        "stack_slots_in_locals: no register form for: {code}"
    );
    body.trim().to_string()
}
