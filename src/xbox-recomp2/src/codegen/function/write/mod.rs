//! Turns one [`FunctionModel`] into a C++ function.
//!
//! The output, in order:
//!
//! 1. An `extern` for each hook: host code run before a guest instruction.
//! 2. The public name and the function's first line.
//! 3. The scratch variables.
//! 4. A `goto` to the start, if the start isn't the first block.
//! 5. Each block: its label, its code, then a fallthrough, `goto` or abort.
//! 6. The closing brace.
//! 7. If a pass changed the calling convention, the original entry.
//!
//! Each instruction becomes a disassembly comment, then a hook call if it
//! has one, then its C++.
//!
//! Comments for added or changed instructions start with `(rewritten)`.

use super::model::{Block, Entry, Exit, FunctionAbi, FunctionModel, GuestOp, Local, Op};
use super::{FunctionScratch, GenerationContext};
use crate::WrongAnalyse;
use crate::codegen::run_state::RunState;
use crate::decode::Mnemonic;
use crate::templates::{
    FASTCALL_SUFFIX, FUNC_BODY_CLOSE, FUNC_PROLOGUE, LABEL, LAST_INST_WARNING_COMMENT,
    LAST_INST_WARNING_PRINTF, MISSING_INSTRUCTION_COMMENT, MISSING_INSTRUCTION_PRINTF, TEMP_DOUBLE,
    TEMP_UINT32, TEMP_UINT64, render_into,
};
use core::fmt::Write as _;
use core::str::from_utf8;
pub(in crate::codegen) use emit::supports_addressing;
use emit::{CodeWriter, Emitter, FunctionState, emit_call, emit_cdq_idiv, emit_return, lookup};
use xbox_recomp2_executable::GuestAddress32;

pub(super) mod emit;
mod fastcall_adapter;
mod locals;
mod merged_cmp_jmp;
mod midasm_hook;
mod nonvolatile_stack;

/// Writes `model` as the C++ function `name`, appending to `out`.
///
/// # Arguments
///
/// * `context` - the executable, config and platform being generated.
/// * `model` - the function to write.
/// * `name` - the function's C++ name.
/// * `scratch` - reusable buffers.
/// * `state` - run-wide reports this function adds to.
/// * `out` - where the C++ goes.
pub(super) fn write(
    context: &GenerationContext<'_>,
    model: &FunctionModel,
    name: &str,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Declare hooks.
    for hook in &model.hooks {
        // Example output: `extern void my_hook(X86Context& __restrict ctx, uint8_t* base);`
        midasm_hook::write_hook_declaration(context.config, hook.address, &mut scratch.text);
        out.extend_from_slice(scratch.text.as_bytes());
    }

    // Prevent host replacements when ABI is changed.
    // Use our speedy nyoom nyoom __fastcall adaptah!
    scratch.text.clear();
    let protected = model.abi == FunctionAbi::Protected;
    let fastcall_name;
    let public_args = if protected {
        fastcall_name = format!("{name}{FASTCALL_SUFFIX}");
        [
            ("name", fastcall_name.as_str()),
            ("linkage", "X86_FUNC_IMPL"),
        ]
    } else {
        [("name", name), ("linkage", unchanged_linkage(model.abi))]
    };
    render_into(FUNC_PROLOGUE, &public_args, &mut scratch.text);
    write_body(context, model, scratch, state, out);
    render_into(FUNC_BODY_CLOSE, &[], &mut scratch.text);
    if protected {
        fastcall_adapter::write(&model.fastcall, name, &mut scratch.text);
    }
    out.extend_from_slice(scratch.text.as_bytes());
}

fn unchanged_linkage(abi: FunctionAbi) -> &'static str {
    if abi == FunctionAbi::Replaced {
        "X86_WEAK_FUNC"
    } else {
        "X86_PROTECTED_FUNC"
    }
}

/// Writes the body of the function, in order:
///
/// - Variables e.g. `uint32_t val_uint32_2 = 0;`
/// - The entry `goto loc_401010;`
/// - Code blocks after the body's opening line.
fn write_body(
    context: &GenerationContext<'_>,
    model: &FunctionModel,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Retrieve the variables to declare at top of recompiled function.
    for local in locals::get_variable_declarations(&model.blocks) {
        write_local(local, &mut scratch.text);
    }
    write_entry(
        model.entry,
        context.base,
        context.config.debug,
        state,
        &mut scratch.text,
    );
    out.extend_from_slice(scratch.text.as_bytes());
    let mut function_state = FunctionState {
        local_targets: model.local_targets(),
        mask_branch_targets: model.mask_branch_targets,
        ebp_frame: model.ebp_frame,
        ..FunctionState::default()
    };
    for block in &model.blocks {
        write_block(context, block, &mut function_state, scratch, state, out);
    }

    scratch.text.clear();
}

/// Writes one block: its label, its operations, then its exit.
///
/// # Examples
///
/// A block at `0x401000` holding `mov eax, ecx`, which then jumps to
/// `0x401010`, comes out as:
///
/// ```text
/// loc_401000:
///     // mov eax, ecx
///     ctx.eax.u32 = ctx.ecx.u32;
///     goto loc_401010;
/// ```
fn write_block(
    context: &GenerationContext<'_>,
    block: &Block,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Label first, so other blocks can `goto` here.
    if let Some(label) = block.label {
        scratch.text.clear();
        render_into(
            LABEL,
            &[("address", &format!("{label:X}"))],
            &mut scratch.text,
        );
        out.extend_from_slice(scratch.text.as_bytes());
    }

    // Then the C++ for each instruction in the block.
    let aborts = matches!(block.exit, Exit::Abort { .. } | Exit::NoFallthrough { .. });
    for op in &block.ops {
        match op {
            Op::Guest(guest) => {
                write_guest(context, guest, aborts, function_state, scratch, state, out);
            }
            // Below are some 'intrinsics', or instructions we give special
            // handling for optimisation purposes.
            Op::Call(call) => {
                write_instruction(
                    context,
                    &call.guest,
                    aborts,
                    function_state,
                    scratch,
                    state,
                    out,
                    |emitter| emit_call(emitter, call),
                );
            }
            Op::Return(ret) => {
                write_instruction(
                    context,
                    &ret.guest,
                    aborts,
                    function_state,
                    scratch,
                    state,
                    out,
                    |emitter| emit_return(emitter, ret),
                );
            }
            Op::CdqIdiv(idiv) => {
                write_instruction(
                    context,
                    &idiv.guest,
                    aborts,
                    function_state,
                    scratch,
                    state,
                    out,
                    emit_cdq_idiv,
                );
            }
            Op::MergedCmpJmp(merged) => {
                merged_cmp_jmp::write(context, merged, aborts, function_state, scratch, state, out);
            }
            Op::Failed { address, failure } => failure.report(*address, state.diag),
        }
    }

    // Finally, how the block ends: like goto, abort, etc.
    scratch.text.clear();
    match block.exit {
        Exit::Next => {}
        Exit::Goto(target) => {
            let _ = writeln!(scratch.text, "\tgoto loc_{target:X};");
        }
        Exit::Abort { message, address } => {
            write_abort(
                message,
                address,
                context.config.debug,
                state,
                &mut scratch.text,
            );
        }
        Exit::NoFallthrough { address } => {
            let message = match block.ops.last() {
                Some(Op::Guest(guest)) if guest.insn.mnemonic() == Mnemonic::INT3 => "INT3 reached",
                Some(Op::Guest(guest)) if lookup(guest.insn.mnemonic()).is_none() => {
                    "Fallthrough after unhandled instruction"
                }
                _ => "Selected block ends without control transfer",
            };
            write_abort(
                message,
                address,
                context.config.debug,
                state,
                &mut scratch.text,
            );
        }
    }
    out.extend_from_slice(scratch.text.as_bytes());
}

/// Writes a jump to the entry, if needed.
///
/// # Examples
///
/// For `Entry::Goto(0x401010)`, this appends:
///
/// ```text
/// goto loc_401010;
/// ```
///
/// # Note
///
/// I (Sewer) looked under disassembler. No penalty for starting with goto,
/// Clang optimises it out.
fn write_entry(
    entry: Entry,
    base: GuestAddress32,
    debug: bool,
    state: &mut RunState<'_>,
    text: &mut String,
) {
    match entry {
        Entry::First => {}
        Entry::Goto(target) => {
            let _ = writeln!(text, "\tgoto loc_{target:X};");
        }
        Entry::Missing(entry) => {
            let _ = writeln!(
                state.diag,
                "ERROR: Function at 0x{base:X} has no selected instruction at its entry 0x{entry:X}"
            );
            if debug {
                let _ = write!(
                    text,
                    "\tfprintf(stderr, \"Function entry 0x{entry:X} was not selected into the body\\n\");\n\tfflush(stderr);\n"
                );
            }
            text.push_str("\tstd::abort();\n");
        }
    }
}

/// Writes the C declaration for one variable.
///
/// # Examples
///
/// For `Local::Uint32(2)`, this appends:
///
/// ```text
/// uint32_t val_uint32_2 = 0;
/// ```
fn write_local(local: Local, text: &mut String) {
    let (template, index) = match local {
        Local::Double(index) => (TEMP_DOUBLE, index),
        Local::Uint32(index) => (TEMP_UINT32, index),
        Local::Uint64(index) => (TEMP_UINT64, index),
    };
    render_into(template, &[("index", &index.to_string())], text);
}

/// Ends a block with `std::abort()` when it has no safe place to go next.
fn write_abort(
    message: &str,
    address: GuestAddress32,
    debug: bool,
    state: &mut RunState<'_>,
    text: &mut String,
) {
    if debug {
        let _ = write!(
            text,
            "\tfprintf(stderr, \"{message} at 0x{address:X}\\n\");\n\tfflush(stderr);\n"
        );
    }
    text.push_str("\tstd::abort();\n");
    let _ = writeln!(
        state.diag,
        "Terminal barrier after instruction at 0x{address:X}: {message}"
    );
}

/// Writes the C++ for one guest instruction.
///
/// For `mov eax, ecx` with a hook named `test_hook`, you get:
///
/// 1. The original instruction, as a comment:
///
///    ```text
///    // mov eax, ecx
///    ```
///
/// 2. The hook call, if the address has a hook:
///
///    ```text
///    test_hook(ctx.eax);
///    ```
///
/// 3. The C++ that does the same work:
///
///    ```text
///    ctx.eax.u32 = ctx.ecx.u32;
///    ```
fn write_guest(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    block_aborts: bool,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // No translation for this instruction; leave a warning and comment
    let Some(emit) = lookup(guest.insn.mnemonic()) else {
        write_instruction_prefix(context, guest, function_state, scratch, state, out);
        write_missing(context, guest, scratch, out);
        report_failure(
            state,
            context.base,
            guest,
            guest.last_in_function && !block_aborts,
        );
        return;
    };

    // Write the C++ that replicates the instruction.
    write_instruction(
        context,
        guest,
        block_aborts,
        function_state,
        scratch,
        state,
        out,
        emit,
    );
}

/// Writes one decoded instruction
#[expect(clippy::too_many_arguments, reason = "nya mrrrp mmew (just happened)")]
fn write_instruction(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    block_aborts: bool,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
    emit: impl FnOnce(&mut Emitter<'_>),
) {
    // Show the instruction, call its hook and count it.
    write_instruction_prefix(context, guest, function_state, scratch, state, out);

    // Write the C++ that replicates the instruction.
    // If the emitter can't handle these operands, report it as missing.
    scratch.code.clear();
    let mut emitter = guest_emitter(context, guest, function_state, &mut scratch.code);
    emit(&mut emitter);
    if emitter.form_missing {
        write_missing(context, guest, scratch, out);
        report_failure(state, context.base, guest, false);
    } else {
        write_code(context, guest, function_state, scratch.code.as_str(), out);
    }

    if guest.last_in_function {
        write_guest_func_end(
            context,
            guest,
            block_aborts,
            function_state,
            scratch,
            state,
            out,
        );
    }
}

/// Borrows the common emission context for one guest instruction.
fn guest_emitter<'a>(
    context: &'a GenerationContext<'_>,
    guest: &'a GuestOp,
    function_state: &'a mut FunctionState,
    out: &'a mut CodeWriter,
) -> Emitter<'a> {
    Emitter {
        out,
        base: guest.address,
        insn: &guest.insn,
        needed_eflags: guest.needed_flags,
        symbols: &context.executable.symbols,
        imports: &context.executable.imports,
        config: context.config,
        platform: context.platform,
        function_state,
        switch_table: guest.switch_table.as_ref(),
        form_missing: false,
    }
}

/// Copies `code`, written for `guest`, applying in-C++ rewrites if required.
fn write_code(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    function_state: &FunctionState,
    code: &str,
    out: &mut Vec<u8>,
) {
    if context.config.speed_hacks.runs_nonvolatile_stack() {
        nonvolatile_stack::rewrite(&guest.insn, function_state.ebp_frame, code, out);
    } else {
        out.extend_from_slice(code.as_bytes());
    }
}

/// Wraps up after the function's last instruction.
fn write_guest_func_end(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    block_aborts: bool,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Reset for next function.
    function_state.has_register_import = false;

    // Warn if the function just stops, since analysis likely cut it short.
    let leaves = matches!(
        guest.insn.mnemonic(),
        Mnemonic::JMP | Mnemonic::RET | Mnemonic::INT3
    );
    if !block_aborts && !leaves {
        scratch.text.clear();
        if context.config.debug {
            render_into(
                LAST_INST_WARNING_PRINTF,
                &[("address", &format!("{:x}", guest.address))],
                &mut scratch.text,
            );
        }
        render_into(LAST_INST_WARNING_COMMENT, &[], &mut scratch.text);
        out.extend_from_slice(scratch.text.as_bytes());
        report_failure(state, context.base, guest, true);
    }
}

/// Writes what goes before an instruction's C++.
///
/// - The instruction, as a disassembly comment.
/// - Its hook call, if any.
///
/// Also counts the instruction for stats.
fn write_instruction_prefix(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    function_state: &mut FunctionState,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Show the instruction as a comment, e.g. `// mov eax, ecx`.
    scratch.text.clear();
    scratch.text.push_str("\t// ");
    if guest.rewritten {
        scratch.text.push_str("(rewritten) ");
    }
    if let Ok(comment) =
        scratch
            .formatter
            .format(&guest.insn, u64::from(guest.address), &mut scratch.disasm)
    {
        scratch.text.push_str(comment);
    }
    scratch.text.push('\n');
    out.extend_from_slice(scratch.text.as_bytes());

    // Call this address's hook, if it has one.
    midasm_hook::write_hook_call(
        context.config,
        guest.address,
        &function_state.local_targets,
        &mut scratch.text,
    );
    out.extend_from_slice(scratch.text.as_bytes());

    // Count the instruction for stats.
    (state.mnemonics)(guest.insn.mnemonic());
}

/// Writes the MISSING comment (and a debug printf) for an untranslated instruction.
/// Call after [`write_instruction_prefix`], which fills `scratch.disasm`.
fn write_missing(
    context: &GenerationContext<'_>,
    guest: &GuestOp,
    scratch: &mut FunctionScratch,
    out: &mut Vec<u8>,
) {
    scratch.text.clear();
    render_into(MISSING_INSTRUCTION_COMMENT, &[], &mut scratch.text);
    if context.config.debug {
        render_into(
            MISSING_INSTRUCTION_PRINTF,
            &[
                ("disasm", disasm_text(&scratch.disasm)),
                ("address", &format!("{:X}", guest.address)),
            ],
            &mut scratch.text,
        );
    }
    out.extend_from_slice(scratch.text.as_bytes());
}

fn disasm_text(buffer: &[u8]) -> &str {
    let end = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    from_utf8(&buffer[..end]).unwrap_or("")
}

/// Notes a bad instruction for end report.
fn report_failure(
    state: &mut RunState<'_>,
    base: GuestAddress32,
    guest: &GuestOp,
    wrong_analyse: bool,
) {
    if wrong_analyse {
        let entry = WrongAnalyse {
            base: u64::from(base),
            address: u64::from(guest.address),
        };
        if state.wrong_analyse_index.insert(entry) {
            state.wrong_analyse.push(entry);
        }
        return;
    }

    let mnemonic = guest.insn.mnemonic().to_string();
    let _ = writeln!(
        state.diag,
        "Unrecognised instruction at 0x{:X}: {mnemonic}",
        guest.address
    );
    if state.inst_missing_index.insert(mnemonic.clone()) {
        state.inst_missing.push(mnemonic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::function::model::ALL_ARITHMETIC_FLAGS;
    use crate::codegen::optimizations::helpers::tests::{PlainPlatform, executable, instruction};
    use crate::config::RecompilerConfig;
    use ahash::HashSet;
    use rstest::rstest;

    // ── Missing forms ────────────────────────────────────────────────────────

    /// An emitter that gives up on a form reports it like an unknown instruction.
    #[rstest]
    #[case::release(false, "\t// psrlq mm0, 0x01\n\t//MISSING INSTRUCTION HERE TODO ADD\n")]
    #[case::debug(
        true,
        "\t// psrlq mm0, 0x01\n\t//MISSING INSTRUCTION HERE TODO ADD\n\tprintf(\"Missing instruction: psrlq mm0, 0x01 - 1000\\n\");\n"
    )]
    fn instruction_should_report_missing_when_emitter_rejects_form(
        #[case] debug: bool,
        #[case] expected: &str,
    ) {
        // Arrange
        // 0F 73 D0 01: psrlq mm0, 1. The emitter writes code, then gives up.
        let code = [0x0F, 0x73, 0xD0, 0x01];
        let guest = GuestOp {
            address: 0x1000.into(),
            insn: instruction(&code),
            switch_table: None,
            last_in_function: false,
            rewritten: false,
            needed_flags: ALL_ARITHMETIC_FLAGS,
        };
        let executable = executable(&code);
        let config = RecompilerConfig {
            debug,
            ..RecompilerConfig::default()
        };
        let context = GenerationContext {
            executable: &executable,
            config: &config,
            platform: &PlainPlatform,
            base: 0x1000.into(),
        };
        let (mut missing, mut missing_index) = (Vec::new(), HashSet::default());
        let (mut wrong, mut wrong_index) = (Vec::new(), HashSet::default());
        let mut diag = Vec::new();
        let mut state = RunState {
            inst_missing: &mut missing,
            inst_missing_index: &mut missing_index,
            wrong_analyse: &mut wrong,
            wrong_analyse_index: &mut wrong_index,
            mnemonics: &mut |_| (),
            diag: &mut diag,
        };
        let mut out = Vec::new();

        // Act
        write_instruction(
            &context,
            &guest,
            false,
            &mut FunctionState::default(),
            &mut FunctionScratch::new(),
            &mut state,
            &mut out,
            |emitter| {
                emitter.out.line("partial();");
                emitter.missing_form();
            },
        );

        // Assert
        assert_eq!(String::from_utf8(out).unwrap(), expected);
        assert_eq!(missing, ["psrlq"]);
    }
}
