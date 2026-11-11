//! Writes a [`MergedCmpJmp`]:
//! - a compare and jumps
//! - SETcc and CMOVcc after it.

use super::emit::{FunctionState, merged_cmp_jmp};
use super::{
    guest_emitter, report_failure, write_code, write_guest, write_guest_func_end,
    write_instruction_prefix, write_missing,
};
use crate::codegen::function::model::{GuestOp, MergedCmpJmp, MergedInstructionKind};
use crate::codegen::function::{FunctionScratch, GenerationContext};
use crate::codegen::run_state::RunState;

/// Writes a compare and the instructions merged with it.
///
/// # Examples
///
/// For these instructions, this appends:
///
/// - `cmp eax, [esi]`
/// - `mov [esi], ecx`
/// - `jz 0x00401010`
///
/// ```text
///     // cmp eax, [esi]
///     {
///     const uint32_t rhs = X86_LOAD_U32(ctx.esi.u32);
///     // mov [esi], ecx
///     X86_STORE_U32(ctx.esi.u32, ctx.ecx.u32);
///     // jz 0x00401010
///     if (ctx.eax.u32 == rhs) goto loc_401010;
///     }
/// ```
pub(super) fn write(
    context: &GenerationContext<'_>,
    merged: &MergedCmpJmp,
    block_aborts: bool,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // // cmp eax, [esi]
    // // fstp st0
    let compare = &merged.compare;
    write_instruction_prefix(context, compare, function_state, scratch, state, out);
    if let Some(pop) = &merged.x87_pop {
        write_instruction_prefix(context, pop, function_state, scratch, state, out);
    }

    // const uint32_t rhs = X86_LOAD_U32(ctx.esi.u32);
    scratch.code.clear();
    let mut emitter = guest_emitter(context, compare, function_state, &mut scratch.code);
    let Some(output) = merged_cmp_jmp::write_compare(&mut emitter, merged) else {
        // Can't happen today: `MergedCmpJmp::accepts` only admits operands we can write.
        write_missing(context, compare, scratch, out);
        report_failure(state, context.base, compare, false);
        return;
    };

    // { <- lets a goto jump past `rhs`
    let scoped = declares_local(scratch.code.as_str());
    if scoped {
        out.extend_from_slice(b"\t{\n");
    }
    write_code(context, compare, scratch.code.as_str(), out);

    // // mov [esi], ecx
    // X86_STORE_U32(ctx.esi.u32, ctx.ecx.u32);
    // // jz 0x00401010
    // if (ctx.eax.u32 == compare_rhs) goto loc_401010;
    for merged_insn in &merged.instructions {
        let guest = &merged_insn.guest;
        if merged_insn.kind == MergedInstructionKind::Plain {
            write_guest(
                context,
                guest,
                block_aborts,
                function_state,
                scratch,
                state,
                out,
            );
        } else {
            write_jump_or_select(
                context,
                merged,
                &output,
                guest,
                merged_insn.kind,
                function_state,
                scratch,
                state,
                out,
            );
        }
    }

    // ctx.eflags.update_compare_flags_U32<0x40>(ctx.eax.u32, rhs);
    // }
    scratch.code.clear();
    let mut emitter = guest_emitter(context, compare, function_state, &mut scratch.code);
    merged_cmp_jmp::write_end_flags(&mut emitter, &output, merged.end_flags);
    write_code(context, compare, scratch.code.as_str(), out);
    if scoped {
        out.extend_from_slice(b"\t}\n");
    }

    let last = &merged
        .instructions
        .last()
        .expect("merged ops end with a jump or select")
        .guest;
    if last.last_in_function {
        write_guest_func_end(
            context,
            last,
            block_aborts,
            function_state,
            scratch,
            state,
            out,
        );
    }
}

/// Whether `code` declares a local, like `const uint32_t old = ...`.
fn declares_local(code: &str) -> bool {
    code.lines()
        .any(|line| line.trim_start().starts_with("const "))
}

/// Writes one merged jump, SETcc or CMOVcc.
///
/// # Examples
///
/// For the `jz` in these instructions, this appends:
///
/// - `cmp eax, ebx`
/// - `jz 0x00401010`
///
/// ```text
///     // jz 0x00401010
///     if (ctx.eax.u32 == ctx.ebx.u32) goto loc_401010;
/// ```
#[expect(
    clippy::too_many_arguments,
    reason = "same state as write_instruction, plus the merge"
)]
fn write_jump_or_select(
    context: &GenerationContext<'_>,
    merged: &MergedCmpJmp,
    output: &merged_cmp_jmp::CompareOutput,
    guest: &GuestOp,
    kind: MergedInstructionKind,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // // jz 0x00401010
    write_instruction_prefix(context, guest, function_state, scratch, state, out);

    // if (ctx.eax.u32 == ctx.ebx.u32) goto loc_401010;
    scratch.code.clear();
    let mut emitter = guest_emitter(context, guest, function_state, &mut scratch.code);
    merged_cmp_jmp::write_jump_or_select(
        &mut emitter,
        merged.compare.insn.mnemonic(),
        output,
        kind,
    );
    if emitter.form_missing {
        write_missing(context, guest, scratch, out);
        report_failure(state, context.base, guest, false);
        return;
    }

    // Emit the code to writer.
    let owner = match kind {
        MergedInstructionKind::Select => guest,
        _ => &merged.compare,
    };
    write_code(context, owner, scratch.code.as_str(), out);
}

#[cfg(test)]
mod tests {
    use crate::analysis::FunctionGraph;
    use crate::codegen::function::model::FunctionModel;
    use crate::codegen::function::write::write;
    use crate::codegen::function::{FunctionScratch, GenerationContext};
    use crate::codegen::optimizations;
    use crate::codegen::optimizations::helpers::tests::{PlainPlatform, executable, model_of};
    use crate::codegen::run_state::RunState;
    use crate::config::{OptimizationHints, Optimizations, RecompilerConfig, SpeedHacks};
    use crate::executable::ImportMap;
    use ahash::HashSet;
    use core::slice;
    use rstest::rstest;

    /// Writes `code` as one optimised function.
    ///
    /// # Returns
    ///
    /// The lines from the first instruction up to `// cmp ecx, edx`, without
    /// indents or labels.
    fn cpp_until_flag_reset(code: &[u8], hacks: SpeedHacks) -> String {
        let model = optimised_model(code, &hacks);
        let cpp = function_cpp(code, &model, hacks);
        merged_lines(&cpp)
    }

    /// Builds `code` as one function, then runs the flag optimisations on it.
    fn optimised_model(code: &[u8], hacks: &SpeedHacks) -> FunctionModel {
        let switches = Optimizations {
            eliminate_dead_flags: true,
            fuse_compare_branch: true,
            ..Optimizations::none()
        };

        let mut model = model_of(code);
        optimizations::run(
            slice::from_mut(&mut model),
            &FunctionGraph::default(),
            &ImportMap::default(),
            &OptimizationHints::default(),
            &switches,
            hacks,
            &mut Vec::new(),
        );
        model
    }

    /// Writes `model` as C++.
    fn function_cpp(code: &[u8], model: &FunctionModel, hacks: SpeedHacks) -> String {
        let executable = executable(code);
        let config = RecompilerConfig {
            speed_hacks: hacks,
            ..RecompilerConfig::default()
        };
        let context = GenerationContext {
            executable: &executable,
            config: &config,
            platform: &PlainPlatform,
            base: model.address.0.into(),
            x87: &model.x87,
        };

        // Ignore failures.
        let (mut missing, mut missing_index) = (Vec::new(), HashSet::default());
        let (mut wrong, mut wrong_index) = (Vec::new(), HashSet::default());
        let mut state = RunState {
            inst_missing: &mut missing,
            inst_missing_index: &mut missing_index,
            wrong_analyse: &mut wrong,
            wrong_analyse_index: &mut wrong_index,
            mnemonics: &mut |_| (),
            diag: &mut Vec::new(),
        };

        let mut out = Vec::new();
        write(
            &context,
            model,
            "f",
            &mut FunctionScratch::new(),
            &mut state,
            &mut out,
        );
        String::from_utf8(out).unwrap()
    }

    /// The lines of `cpp` from the first instruction up to `// cmp ecx, edx`.
    fn merged_lines(cpp: &str) -> String {
        cpp.lines()
            .skip_while(|line| !line.starts_with("\t// ")) // <- skip the function header
            .take_while(|line| *line != "\t// cmp ecx, edx") // <- stop at the flag reset
            .filter(|line| !line.starts_with("loc_"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `expected` with each line's indent removed.
    fn unindent(expected: &str) -> String {
        expected
            .lines()
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[rstest]
    #[case::dec(
        &[
            0x49,                   //     dec  ecx
            0x75, 0,                //     jnz  .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// dec ecx
         ctx.ecx.u32 -= 1;
         // jnz 0x00001003
         if (ctx.ecx.u32 != 0) goto loc_1003;",
    )]
    #[case::sub_relation(
        &[
            0x29, 0xD8,             //     sub  eax, ebx
            0x72, 0,                //     jb   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// sub eax, ebx
         {
         const uint32_t old = ctx.eax.u32;
         ctx.eax.u32 = old - ctx.ebx.u32;
         // jb 0x00001004
         if (old < ctx.ebx.u32) goto loc_1004;
         }",
    )]
    #[case::sub_same_register(
        &[
            0x29, 0xC0,             //     sub  eax, eax
            0x72, 0,                //     jb   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// sub eax, eax
         {
         const uint32_t source = ctx.eax.u32;
         const uint32_t old = ctx.eax.u32;
         ctx.eax.u32 = old - source;
         // jb 0x00001004
         if (old < source) goto loc_1004;
         }",
    )]
    #[case::add_carry(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x72, 0,                //     jb   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// add eax, ebx
         {
         const uint32_t old = ctx.eax.u32;
         ctx.eax.u32 = old + ctx.ebx.u32;
         // jb 0x00001004
         if (ctx.eax.u32 < old) goto loc_1004;
         }",
    )]
    #[case::and_signed(
        &[
            0x21, 0xD8,             //     and  eax, ebx
            0x7E, 0,                //     jle  .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// and eax, ebx
         ctx.eax.u32 &= ctx.ebx.u32;
         // jle 0x00001004
         if (static_cast<int32_t>(ctx.eax.u32) <= 0) goto loc_1004;",
    )]
    #[case::byte_register(
        &[
            0xFE, 0xC8,             //     dec  al
            0x78, 0,                //     js   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// dec al
         ctx.eax.u32 = (ctx.eax.u32 & 0xFFFFFF00u) | (uint8_t)(ctx.eax.u8_lo - 1);
         // js 0x00001004
         if (static_cast<int8_t>(ctx.eax.u8_lo) < 0) goto loc_1004;",
    )]
    #[case::memory(
        &[
            0xFF, 0x0E,             //     dec  dword ptr [esi]
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// dec [esi]
         {
         const uint32_t result = X86_NONVOLATILE_LOAD_U32(static_cast<uintptr_t>(ctx.esi.u32)) - 1;
         X86_NONVOLATILE_STORE_U32(static_cast<uintptr_t>(ctx.esi.u32), result);
         // jz 0x00001004
         if (result == 0) goto loc_1004;
         }",
    )]
    #[case::sar(
        &[
            0xC0, 0xF8, 2,          //     sar  al, 2
            0x7A, 0,                //     jp   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// sar al, 0x02
         ctx.eax.u32 = (ctx.eax.u32 & 0xFFFFFF00u) | (uint8_t)(static_cast<int8_t>(ctx.eax.u8_lo) >> 2);
         // jp 0x00001005
         if (__builtin_parity(ctx.eax.u8_lo) == 0) goto loc_1005;",
    )]
    #[case::sahf(
        &[
            0x9E,                   //     sahf
            0x76, 0,                //     jbe  .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// sahf
         // jbe 0x00001003
         if ((ctx.eax.u8_hi & (X86EFlags::CF | X86EFlags::ZF)) != 0) goto loc_1003;",
    )]
    #[case::test_same(
        &[
            0x85, 0xDB,             //     test ebx, ebx
            0xBB, 2, 0, 0, 0,       //     mov  ebx, 2
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// test ebx, ebx
         {
         const uint32_t value = ctx.ebx.u32;
         // mov ebx, 0x02
         ctx.ebx.u32 = 0x2u;
         // jz 0x00001009
         if (value == 0) goto loc_1009;
         }",
    )]
    #[case::cmp_byte_constant(
        &[
            0x3C, 0xFF,             //     cmp  al, -1
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// cmp al, 0xFF
         // jz 0x00001004
         if (ctx.eax.u8_lo == 0xFFu) goto loc_1004;",
    )]
    #[case::dec_flags_read(
        &[
            0x49,                   //     dec  ecx
            0x74, 3,                //     jz   .setz
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
                                    // .setz:
            0x0F, 0x94, 0xC0,       //     setz al
            0xC3,                   //     ret
        ],
        "// dec ecx
         {
         const uint32_t old = ctx.ecx.u32;
         ctx.ecx.u32 = old - 1;
         // jz 0x00001006
         if (ctx.ecx.u32 == 0) {
         ctx.eflags.update_flags_dec_U32<0x40>(old, ctx.ecx.u32);
         goto loc_1006;
         }
         }",
    )]
    #[case::xor_flags_read(
        &[
            0x31, 0xD8,             //     xor  eax, ebx
            0x74, 3,                //     jz   .setz
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
                                    // .setz:
            0x0F, 0x94, 0xC0,       //     setz al
            0xC3,                   //     ret
        ],
        "// xor eax, ebx
         ctx.eax.u32 ^= ctx.ebx.u32;
         // jz 0x00001007
         if (ctx.eax.u32 == 0) {
         ctx.eflags.update_flags_logic<0x40>(ctx.eax.u32);
         goto loc_1007;
         }",
    )]
    #[case::float_same_flags(
        &[
            0x0F, 0x2F, 0xC1,       //     comiss xmm0, xmm1
            0x72, 2,                //     jb   .end
            0x73, 0,                //     jnb  .end      ; same flag as jb
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// comiss xmm0, xmm1
         {
         const bool compare_taken = ctx.branch_ss<true, false, 0x1>(_mm_cvtss_f32(ctx.xmm[0].reg), _mm_cvtss_f32(ctx.xmm[1].reg));
         // jb 0x00001007
         if (compare_taken) goto loc_1007;
         // jnb 0x00001007
         if (!compare_taken) goto loc_1007;
         }",
    )]
    #[case::float_other_flags(
        &[
            0x0F, 0x2F, 0xC1,       //     comiss xmm0, xmm1
            0x72, 2,                //     jb   .end
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// comiss xmm0, xmm1
         {
         const uint32_t compare_flags = ctx.compare_flags_ss<true>(_mm_cvtss_f32(ctx.xmm[0].reg), _mm_cvtss_f32(ctx.xmm[1].reg));
         // jb 0x00001007
         if ((compare_flags & X86EFlags::CF) != 0) goto loc_1007;
         // jz 0x00001007
         if ((compare_flags & X86EFlags::ZF) != 0) goto loc_1007;
         }",
    )]
    #[case::fcomip_other_flags(
        &[
            0xDF, 0xF1,             //     fcomip st0, st1
            0x72, 2,                //     jb   .end
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// fcomip st0, st1
         {
         val_double_0 = ctx.fpu.st1();
         const uint32_t fcomi_flags = x86_float_flags(ctx.fpu.st0(), val_double_0);
         ctx.fpu.pop<false>();
         // jb 0x00001006
         if ((fcomi_flags & X86EFlags::CF) != 0) goto loc_1006;
         // jz 0x00001006
         if ((fcomi_flags & X86EFlags::ZF) != 0) goto loc_1006;
         }",
    )]
    fn write_should_merge_with_jump(#[case] code: &[u8], #[case] expected: &str) {
        // Act
        let actual = cpp_until_flag_reset(code, SpeedHacks::default());

        // Assert
        assert_eq!(actual, unindent(expected));
    }

    /// With status flags tracked, readers of different flags test eflags.
    #[rstest]
    #[case::sse(
        &[
            0x0F, 0x2F, 0xC1,       //     comiss xmm0, xmm1
            0x72, 2,                //     jb   .end
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// comiss xmm0, xmm1
         ctx.compare_ss<true>(_mm_cvtss_f32(ctx.xmm[0].reg), _mm_cvtss_f32(ctx.xmm[1].reg));
         // jb 0x00001007
         if ((ctx.eflags.common & X86EFlags::CF) != 0) goto loc_1007;
         // jz 0x00001007
         if ((ctx.eflags.common & X86EFlags::ZF) != 0) goto loc_1007;",
    )]
    #[case::x87(
        &[
            0xDF, 0xF1,             //     fcomip st0, st1
            0x72, 2,                //     jb   .end
            0x74, 0,                //     jz   .end
                                    // .end:
            0x39, 0xD1,             //     cmp  ecx, edx
            0xC3,                   //     ret
        ],
        "// fcomip st0, st1
         val_double_0 = ctx.fpu.st1();
         ctx.fpu.compare_and_set_eflags(ctx.fpu.st0(), val_double_0, ctx.eflags);
         ctx.fpu.pop<false>();
         // jb 0x00001006
         if ((ctx.eflags.common & X86EFlags::CF) != 0) goto loc_1006;
         // jz 0x00001006
         if ((ctx.eflags.common & X86EFlags::ZF) != 0) goto loc_1006;",
    )]
    fn write_should_test_eflags_when_status_tracked(#[case] code: &[u8], #[case] expected: &str) {
        // Arrange
        let hacks = SpeedHacks {
            fast_sse_status: false,
            fast_x87_status: false,
            ..SpeedHacks::default()
        };

        // Act
        let actual = cpp_until_flag_reset(code, hacks);

        // Assert
        assert_eq!(actual, unindent(expected));
    }
}
