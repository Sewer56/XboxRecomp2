//! The `nonvolatile_stack` speed hack.
//!
//! Makes guest stack accesses plain C++, so the compiler can keep stack values
//! in registers.
//!
//! For an ESP-relative operand:
//!
//! | Before                            | After                                   |
//! | --------------------------------- | --------------------------------------- |
//! | `X86_LOAD_U32(ctx.esp.u32 + 0x4)` | `X86_STACK_LOAD_U32(ctx.esp.u32 + 0x4)` |
//! | `X86_ADD32_MEM(...)`              | `X86_ADD32_MEM<false>(...)`             |
//! | `X86_ADD32_MEM<true, 0x40>(...)`  | `X86_ADD32_MEM<false, 0x40>(...)`       |

use crate::decode::{Insn, OperandVisibility, Register, memory};

/// Load and store macros that have an `X86_STACK_` version.
const MACROS: [&str; 18] = [
    "X86_LOAD_U8",
    "X86_LOAD_U16",
    "X86_LOAD_U32",
    "X86_LOAD_U64",
    "X86_LOAD_F32",
    "X86_LOAD_F64",
    "X86_LOAD_F80",
    "X86_LOAD_PS",
    "X86_LOAD_APS",
    "X86_STORE_U8",
    "X86_STORE_U16",
    "X86_STORE_U32",
    "X86_STORE_U64",
    "X86_STORE_F32",
    "X86_STORE_F64",
    "X86_STORE_F80",
    "X86_STORE_PS",
    "X86_STORE_APS",
];

/// Copies `code` into `out` with its stack accesses made non-volatile.
///
/// # Arguments
///
/// - `insn`: e.g. `mov [esp+4], eax`.
/// - `ebp_frame`: whether `[ebp+N]` is a stack access too.
/// - `code`: its C++, e.g. `X86_STORE_U32(ctx.esp.u32 + 0x4, ctx.eax.u32);`.
/// - `out`: output to append to.
///
/// # Examples
///
/// ```cpp
/// // mov [esp+4], eax
/// X86_STORE_U32(ctx.esp.u32 + 0x4, ctx.eax.u32);       // before
/// X86_STACK_STORE_U32(ctx.esp.u32 + 0x4, ctx.eax.u32); // after
///
/// // mov eax, [ebp+8], with `ebp_frame`
/// ctx.eax.u32 = X86_LOAD_U32(ctx.ebp.u32 + 0x8);       // before
/// ctx.eax.u32 = X86_STACK_LOAD_U32(ctx.ebp.u32 + 0x8); // after
///
/// // add [esp+4], eax
/// X86_ADD32_MEM(ctx.esp.u32 + 0x4, ctx.eax.u32, ctx, base);        // before
/// X86_ADD32_MEM<false>(ctx.esp.u32 + 0x4, ctx.eax.u32, ctx, base); // after
///
/// // add [esp+4], eax, when only ZF is read afterwards
/// X86_ADD32_MEM<true, 0x40>(ctx.esp.u32 + 0x4, ctx.eax.u32, ctx, base);  // before
/// X86_ADD32_MEM<false, 0x40>(ctx.esp.u32 + 0x4, ctx.eax.u32, ctx, base); // after
///
/// // push [eax+0xC]: the push is on the stack, the load isn't
/// X86_PUSH(ctx, base, X86_LOAD_U32(ctx.eax.u32 + 0xC));        // before
/// X86_PUSH<false>(ctx, base, X86_LOAD_U32(ctx.eax.u32 + 0xC)); // after
/// ```
pub(super) fn rewrite(insn: &Insn, ebp_frame: bool, code: &str, out: &mut Vec<u8>) {
    let on_stack = is_stack_relative(insn, ebp_frame);

    let mut rest = code;
    while let Some(start) = next_helper(rest) {
        // Copy the text before the helper.
        out.extend_from_slice(&rest.as_bytes()[..start]);

        // Split off the helper's name.
        // e.g. `X86_ADD32_MEM<true, 0x40>(...)` -> `X86_ADD32_MEM` + `<true, 0x40>(...)`
        let tail = &rest[start..];
        let end = tail.find(|c: char| !is_identifier(c)).unwrap_or(tail.len());
        let (name, after) = tail.split_at(end);
        rest = after;

        // Write the name, using the `X86_STACK_` version of loads and stores.
        if on_stack && MACROS.contains(&name) {
            out.extend_from_slice(b"X86_STACK_");
            out.extend_from_slice(&name.as_bytes()["X86_".len()..]);
        } else {
            out.extend_from_slice(name.as_bytes());
        }

        // Should we set `Volatile` to false?
        if !should_clear_volatile(name, on_stack) {
            continue;
        }

        // `<true, mask>` from `Emitter::flag_template` becomes `<false, mask>`.
        if let Some(mask) = after.strip_prefix("<true,") {
            out.extend_from_slice(b"<false,");
            rest = mask;
        } else if after.starts_with('(') {
            // No template arguments yet, so add `<false>`.
            out.extend_from_slice(b"<false>");
        }
    }

    out.extend_from_slice(rest.as_bytes());
}

/// Finds the start of the next helper we might rewrite.
fn next_helper(text: &str) -> Option<usize> {
    next_x86_helper(text)
        .into_iter()
        .chain(next_fpu_call(text))
        .min()
}

/// Whether to set `name`'s `Volatile` template parameter to `false`.
fn should_clear_volatile(name: &str, on_stack: bool) -> bool {
    match name {
        // keep in sync with x86_context_core.h
        "X86_PUSH" | "X86_POP" | "X86_PUSH16" | "X86_POP16" => true,
        "save" | "load" => on_stack,
        _ => on_stack && name.ends_with("_MEM") && !name.starts_with("X86_CMPXCHG"),
    }
}

/// Whether `insn`'s visible memory operand is a stack access.
///
/// That's `[esp+N]`, or `[ebp+N]` with `ebp_frame`.
fn is_stack_relative(insn: &Insn, ebp_frame: bool) -> bool {
    let count = usize::from(insn.instruction().operand_count);
    insn.operands()[..count]
        .iter()
        .filter(|operand| operand.visibility != OperandVisibility::HIDDEN)
        .filter_map(memory)
        .any(|mem| {
            let ebp_slot = ebp_frame && mem.base == Register::EBP && mem.index == Register::NONE;
            let stack_base = mem.base == Register::ESP || ebp_slot;
            stack_base && mem.segment == Register::SS
        })
}

/// Finds the `save` or `load` in the next `ctx.fpu.save(` or `ctx.fpu.load(`.
fn next_fpu_call(text: &str) -> Option<usize> {
    let prefix = "ctx.fpu.";
    text.match_indices(prefix)
        .map(|(at, _)| at + prefix.len())
        .find(|&at| text[at..].starts_with("save(") || text[at..].starts_with("load("))
}

/// Finds the next `X86_` name that isn't part of a longer identifier.
fn next_x86_helper(text: &str) -> Option<usize> {
    text.match_indices("X86_")
        .map(|(at, _)| at)
        .find(|&at| !text[..at].ends_with(is_identifier))
}

fn is_identifier(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
