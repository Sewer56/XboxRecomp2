//! Points stack accesses at the local `frame` array, for `stack_slots_in_locals`.
//!
//! | Before                                           | After                                          |
//! | ------------------------------------------------ | ---------------------------------------------- |
//! | `X86_LOAD_U32(ctx.esp.u32 + 0x14)`               | `X86_FRAME_LOAD(uint32_t, 0x1C)`               |
//! | `X86_STORE_U32(ctx.esp.u32 + 0x14, ctx.eax.u32)` | `X86_FRAME_STORE(uint32_t, 0x1C, ctx.eax.u32)` |
//! | `X86_PUSH(ctx, base, ctx.esi.u32)`               | `X86_FRAME_PUSH(ctx, frame, 0x8, ctx.esi.u32)` |

use super::emit::ea;
use crate::codegen::function::model::{FrameAccess, StackLocals};
use crate::decode::Insn;
use core::fmt::Write as _;

/// Load/store macro suffixes and their C++ types.
const MACROS: [(&str, &str); 8] = [
    ("U8", "uint8_t"),
    ("U16", "uint16_t"),
    ("U32", "uint32_t"),
    ("U64", "uint64_t"),
    ("F32", "float"),
    ("F64", "double"),
    ("PS", "x86_guest_m128_u"),
    ("APS", "x86_guest_m128"),
];

/// Macro prefixes.
const PREFIXES: [&str; 2] = ["X86_", "X86_NONVOLATILE_"];

/// Writes `code` to `out`, with `insn`'s stack accesses pointed at `frame`.
///
/// # Arguments
///
/// - `access`: `insn`'s frame offsets.
/// - `insn`: the instruction `code` implements.
/// - `native`: whether `code` uses `native_sized_addresses`.
/// - `code`: the instruction's C++.
/// - `out`: replaced with the result.
pub(super) fn rewrite(
    access: &FrameAccess,
    insn: &Insn,
    native: bool,
    code: &str,
    out: &mut String,
) {
    out.clear();
    out.push_str(code);

    // Memory operands.
    for &(index, offset) in &access.operands {
        let address = ea(&insn.operands()[usize::from(index)], native);
        point_operand_at_frame(out, &address, offset);
        assert!(
            !names_guest_address(out, &address),
            "stack_slots_in_locals: no rewrite for `{address}` in: {code}"
        );
    }

    // Push or pop slot.
    if let Some(offset) = access.push_pop {
        let replaced = point_push_pop_at_frame(out, offset);
        assert_eq!(
            replaced, 1,
            "stack_slots_in_locals: expected one push or pop in: {code}"
        );
    }
}

/// Declares `frame` and fills it from guest memory.
///
/// # Examples
///
/// ```cpp
/// alignas(16) uint8_t frame[0x20];
/// X86_FRAME_ENTER<0x20>(frame, ctx.esp.u32 - 0x20u, base);
/// ```
pub(super) fn write_prologue(locals: &StackLocals, text: &mut String) {
    let size = locals.size;
    let _ = writeln!(text, "\talignas(16) uint8_t frame[0x{size:X}];");
    let _ = writeln!(
        text,
        "\tX86_FRAME_ENTER<0x{size:X}>(frame, ctx.esp.u32 - 0x{size:X}u, base);"
    );
}

/// Rewrites loads and stores of `address`.
fn point_operand_at_frame(text: &mut String, address: &str, offset: u32) {
    for prefix in PREFIXES {
        for (suffix, ty) in MACROS {
            replace_text(
                text,
                &format!("{prefix}LOAD_{suffix}({address})"),
                &format!("X86_FRAME_LOAD({ty}, 0x{offset:X})"),
            );
            replace_text(
                text,
                &format!("{prefix}STORE_{suffix}({address}, "),
                &format!("X86_FRAME_STORE({ty}, 0x{offset:X}, "),
            );
        }
    }

    // MMX loads have their own macro.
    replace_text(
        text,
        &format!("X86_MMX_LOAD({address})"),
        &format!("X86_FRAME_LOAD(uint64_t, 0x{offset:X})"),
    );
}

/// Rewrites `X86_PUSH` and `X86_POP`. Returns how many.
fn point_push_pop_at_frame(text: &mut String, offset: u32) -> usize {
    let pushes = replace_text(
        text,
        "X86_PUSH(ctx, base, ",
        &format!("X86_FRAME_PUSH(ctx, frame, 0x{offset:X}, "),
    );
    let pops = replace_text(
        text,
        "X86_POP(ctx, base)",
        &format!("X86_FRAME_POP(ctx, frame, 0x{offset:X})"),
    );
    pushes + pops
}

/// Replaces every `from` with `to`. Returns how many.
fn replace_text(text: &mut String, from: &str, to: &str) -> usize {
    let count = text.matches(from).count();
    if count != 0 {
        *text = text.replace(from, to);
    }
    count
}

/// Whether a macro still takes `address` as its first argument.
fn names_guest_address(text: &str, address: &str) -> bool {
    text.contains(&format!("({address})")) || text.contains(&format!("({address},"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::instruction;
    use rstest::rstest;

    #[rstest]
    #[case::load(
        &[0x8B, 0x44, 0x24, 0x14], // mov eax, [esp+0x14]
        "ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x14);",
        &[(1, 0x1C)],
        None,
        "ctx.eax.u32 = X86_FRAME_LOAD(uint32_t, 0x1C);",
    )]
    #[case::store(
        &[0x89, 0x44, 0x24, 0x14], // mov [esp+0x14], eax
        "X86_STORE_U32(ctx.esp.u32 + 0x14, ctx.eax.u32);",
        &[(0, 0x1C)],
        None,
        "X86_FRAME_STORE(uint32_t, 0x1C, ctx.eax.u32);",
    )]
    #[case::sse_load(
        &[0x0F, 0x28, 0x44, 0x24, 0x14], // movaps xmm0, [esp+0x14]
        "ctx.xmm[0].reg = X86_NONVOLATILE_LOAD_APS(ctx.esp.u32 + 0x14);",
        &[(1, 0x1C)],
        None,
        "ctx.xmm[0].reg = X86_FRAME_LOAD(x86_guest_m128, 0x1C);",
    )]
    #[case::push(
        &[0x56], // push esi
        "X86_PUSH(ctx, base, ctx.esi.u32);",
        &[],
        Some(0x8),
        "X86_FRAME_PUSH(ctx, frame, 0x8, ctx.esi.u32);",
    )]
    #[case::pop(
        &[0x5E], // pop esi
        "ctx.esi.u32 = X86_POP(ctx, base);",
        &[],
        Some(0x8),
        "ctx.esi.u32 = X86_FRAME_POP(ctx, frame, 0x8);",
    )]
    #[case::leave(
        &[0xC9], // leave
        "ctx.esp.u32 = ctx.ebp.u32;\nctx.ebp.u32 = X86_POP(ctx, base);",
        &[],
        Some(0x8),
        "ctx.esp.u32 = ctx.ebp.u32;\nctx.ebp.u32 = X86_FRAME_POP(ctx, frame, 0x8);",
    )]
    #[case::push_of_slot(
        &[0xFF, 0x34, 0x24], // push [esp]
        "X86_PUSH(ctx, base, X86_LOAD_U32(ctx.esp.u32));",
        &[(0, 0xC)],
        Some(0x8),
        "X86_FRAME_PUSH(ctx, frame, 0x8, X86_FRAME_LOAD(uint32_t, 0xC));",
    )]
    fn rewrite_should_point_slots_at_frame(
        #[case] code: &[u8],
        #[case] cpp: &str,
        #[case] operands: &[(u8, u32)],
        #[case] push_pop: Option<u32>,
        #[case] expected: &str,
    ) {
        // Arrange
        let access = FrameAccess {
            operands: operands.to_vec(),
            push_pop,
        };
        let mut out = String::new();

        // Act
        rewrite(&access, &instruction(code), false, cpp, &mut out);

        // Assert
        assert_eq!(out, expected);
    }
}
