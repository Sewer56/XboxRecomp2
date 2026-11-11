//! Points stack accesses at the local `frame` array, for `stack_slots_in_locals`.
//!
//! | Before                                           | After                                          |
//! | ------------------------------------------------ | ---------------------------------------------- |
//! | `X86_LOAD_U32(ctx.esp.u32 + 0x14)`               | `X86_FRAME_LOAD(uint32_t, 0x1C)`               |
//! | `X86_STORE_U32(ctx.esp.u32 + 0x14, ctx.eax.u32)` | `X86_FRAME_STORE(uint32_t, 0x1C, ctx.eax.u32)` |
//! | `X86_PUSH(ctx, base, ctx.esi.u32)`               | `X86_FRAME_PUSH(ctx, frame, 0x8, ctx.esi.u32)` |
//! | `X86_AND32_MEM(ctx.esp.u32, 0xff, ctx, base)`    | `X86_AND32_MEM<false>(0x8, 0xff, ctx, frame)`  |
//! | `base + (ctx.esp.u32)`                           | `frame + 0x8`                                  |

use super::emit::ea;
use crate::codegen::function::model::{FrameAccess, StackLocals};
use crate::decode::{Insn, Mnemonic};
use core::fmt::Write as _;

/// Load/store macro suffixes and their C++ types.
const MACROS: [(&str, &str); 10] = [
    ("U8", "uint8_t"),
    ("U16", "uint16_t"),
    ("U32", "uint32_t"),
    ("U64", "uint64_t"),
    ("F32", "float"),
    ("F64", "double"),
    ("PS", "x86_guest_m128_u"),
    ("APS", "x86_guest_m128"),
    ("NTPS", "x86_guest_m128"),
    ("NTQ", "uint64_t"),
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
        if insn.mnemonic() == Mnemonic::SGDT {
            point_operand_at_frame(out, &format!("{address}+2"), offset + 2);
        }

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
        replace_text(
            text,
            &format!("{prefix}LOAD_F80({address})"),
            &format!("X86_FRAME_LOAD_F80(0x{offset:X})"),
        );
        replace_text(
            text,
            &format!("{prefix}STORE_F80({address}, "),
            &format!("X86_FRAME_STORE_F80(0x{offset:X}, "),
        );

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

    // Raw pointers: `base + (addr)` -> `frame + 0xC`
    replace_text(
        text,
        &format!("base + ({address})"),
        &format!("frame + 0x{offset:X}"),
    );

    point_helpers_at_frame(text, address, offset);
}

/// Rewrites `_MEM` helpers, like `X86_AND32_MEM`, to use `frame`.
///
/// | Before                                                | After                                                  |
/// | ----------------------------------------------------- | ------------------------------------------------------ |
/// | `X86_AND32_MEM(ctx.esp.u32 + 0x4, 0xff, ctx, base)`   | `X86_AND32_MEM<false>(0xC, 0xff, ctx, frame)`          |
/// | `X86_DEC32_MEM<true, 0x40>(ctx.esp.u32, ctx, base)`   | `X86_DEC32_MEM<false, 0x40>(0x8, ctx, frame)`          |
fn point_helpers_at_frame(text: &mut String, address: &str, offset: u32) {
    let call = format!("({address}, ");
    let mut search = 0;
    while let Some(found) = text[search..].find(&call) {
        let open = search + found;
        search = open + 1;

        // Find `X86_DEC32_MEM` in `X86_DEC32_MEM<true, 0x40>(`.
        let (name_end, template) = match text[..open].strip_suffix('>') {
            Some(before) => match before.rfind('<') {
                Some(start) => (start, Some(&text[start + 1..open - 1])),
                None => continue,
            },
            None => (open, None),
        };

        let name_start = text[..name_end]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |at| at + 1);

        if !text[name_start..name_end].ends_with("_MEM") {
            continue;
        }

        // Find the `base` argument.
        // e.g. `ctx, base)` or `base, true)` for `X86_CVTSS2SI_MEM`.
        let close = match closing_paren(text, open) {
            Some(close) => close,
            None => continue,
        };
        let args = &text[open..close + 1];
        let base = match args.rfind(", base)").or_else(|| args.rfind(", base,")) {
            Some(at) => open + at + ", ".len(),
            None => continue,
        };

        // `<true, 0x40>` -> `<false, 0x40>`
        // ``             -> `<false>`
        let template = match template.and_then(|args| args.split_once(',')) {
            Some((_, mask)) => format!("<false,{mask}>"),
            None => "<false>".to_string(),
        };

        // Now replace from the back
        text.replace_range(base..base + "base".len(), "frame");
        text.replace_range(open + 1..open + 1 + address.len(), &format!("0x{offset:X}"));
        text.replace_range(name_end..open, &template);
        search = name_end;
    }
}

/// Index of the `)` that closes the `(` at `open`.
fn closing_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + at);
                }
            }
            _ => {}
        }
    }

    None
}

/// Rewrites `X86_PUSH` and `X86_POP`. Returns how many.
fn point_push_pop_at_frame(text: &mut String, offset: u32) -> usize {
    let mut count = 0;
    for suffix in ["", "16"] {
        count += replace_text(
            text,
            &format!("X86_PUSH{suffix}(ctx, base, "),
            &format!("X86_FRAME_PUSH{suffix}(ctx, frame, 0x{offset:X}, "),
        );
        count += replace_text(
            text,
            &format!("X86_POP{suffix}(ctx, base)"),
            &format!("X86_FRAME_POP{suffix}(ctx, frame, 0x{offset:X})"),
        );
    }

    count
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
    #[case::helper(
        &[0x83, 0x24, 0x24, 0x0F], // and [esp], 0xF
        "X86_AND32_MEM(ctx.esp.u32, 0xf, ctx, base);",
        &[(0, 0xC)],
        None,
        "X86_AND32_MEM<false>(0xC, 0xf, ctx, frame);",
    )]
    #[case::helper_with_flags(
        &[0xFF, 0x0C, 0x24], // dec [esp]
        "X86_DEC32_MEM<true, 0x40>(ctx.esp.u32, ctx, base);",
        &[(0, 0xC)],
        None,
        "X86_DEC32_MEM<false, 0x40>(0xC, ctx, frame);",
    )]
    #[case::helper_base_not_last(
        &[0xF3, 0x0F, 0x2C, 0x04, 0x24], // cvttss2si eax, [esp]
        "X86_CVTSS2SI_MEM(ctx.esp.u32, ctx.eax.u32, base, true);",
        &[(1, 0xC)],
        None,
        "X86_CVTSS2SI_MEM<false>(0xC, ctx.eax.u32, frame, true);",
    )]
    #[case::raw_pointer(
        &[0x66, 0x0F, 0xFD, 0x04, 0x24], // paddw xmm0, [esp]
        "_mm_loadu_si128(reinterpret_cast<const __m128i*>(base + (ctx.esp.u32)));",
        &[(1, 0xC)],
        None,
        "_mm_loadu_si128(reinterpret_cast<const __m128i*>(frame + 0xC));",
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
