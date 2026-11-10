//! Writes the C++ for mid-assembly hooks.
//!
//! A mid-assembly hook is a user function that the generated code calls
//! just before a chosen instruction.
//!
//! This module writes two things:
//!
//! - the `extern` declaration: [`write_hook_declaration`].
//!   - `extern void test_hook(X86Context& __restrict ctx, uint8_t* base);`
//! - the call: [`write_hook_call`].
//!   - `test_hook(ctx, base);`

use crate::analysis::LocalTargets;
use crate::codegen::function::model::HookSync;
use crate::config::{FunctionAddress, MidAsmHook, RecompilerConfig};
use crate::templates::{MIDASM_HOOK_DECL, render_into};
use core::fmt::Write as _;
use xbox_recomp2_executable::GuestAddress32;

/// Replaces `text` with the call to the hook at `address`, or empties it.
///
/// ```cpp
///     test_hook(ctx, base);
///     goto loc_1000;
/// ```
///
/// When `x87` is `Some`, the function's local `fpu` copy is synced around the hook:
///
/// ```cpp
///     x87_out<1>(ctx.fpu, fpu);
///     test_hook(ctx, base);
///     x87_in<1>(fpu, ctx.fpu);
///     goto loc_1000;
/// ```
pub(super) fn write_hook_call(
    config: &RecompilerConfig,
    address: GuestAddress32,
    local_targets: &LocalTargets,
    x87: Option<HookSync>,
    text: &mut String,
) {
    text.clear();

    // If there's no hook here, yeet out.
    let Some(hook) = config.midasm_hooks.get(FunctionAddress(address.get())) else {
        return;
    };

    let is_local = &|target: GuestAddress32| local_targets.contains(target);

    // Hand the local copy to the hook.
    if let Some(sync) = x87
        && sync.local_to_ctx
    {
        let _ = writeln!(text, "\tx87_out<{}>(ctx.fpu, fpu);", sync.depth);
    }

    if hook_returns_bool(hook) {
        // if (hook(ctx, base)) {
        // or
        // if (x87_after_hook<1>(hook(ctx, base), fpu, ctx.fpu)) {
        text.push_str("\tif (");
        match x87 {
            Some(sync) => {
                let _ = write!(text, "x87_after_hook<{}>(", sync.depth);
                push_call(text, hook);
                text.push_str(", fpu, ctx.fpu)");
            }
            None => push_call(text, hook),
        }
        text.push_str(") {\n");

        // 'return'
        // or
        // 'goto loc_<address>'
        push_after_hook(
            text,
            hook.return_on_true,
            hook.jump_address_on_true,
            "\t\t",
            is_local,
            config.debug,
        );

        // } else {
        text.push_str("\t}\n\telse {\n");

        // 'return'
        // or
        // 'goto loc_<address>'
        push_after_hook(
            text,
            hook.return_on_false,
            hook.jump_address_on_false,
            "\t\t",
            is_local,
            config.debug,
        );
        text.push_str("\t}\n");
    } else {
        // `test_hook(ctx, base)`
        text.push('\t');
        push_call(text, hook);
        text.push_str(";\n");

        // Take back what the hook changed.
        if let Some(sync) = x87 {
            let _ = writeln!(text, "\tx87_in<{}>(fpu, ctx.fpu);", sync.depth);
        }

        // 'return'
        // or
        // 'goto loc_<address>'
        push_after_hook(
            text,
            hook.ret,
            hook.jump_address,
            "\t",
            is_local,
            config.debug,
        );
    }
}

/// Writes the `extern` declaration for the hook at `address` into `text`.
///
/// Example: `extern bool testhook(X86Context& __restrict ctx, uint8_t* base);`
pub(super) fn write_hook_declaration(
    config: &RecompilerConfig,
    address: GuestAddress32,
    text: &mut String,
) {
    // If no hook here, yank out.
    let Some(hook) = config.midasm_hooks.get(FunctionAddress(address.get())) else {
        return;
    };

    // Get return type.
    let returns = if hook_returns_bool(hook) {
        "bool"
    } else {
        "void"
    };

    text.clear();
    render_into(
        MIDASM_HOOK_DECL,
        &[("returns", returns), ("name", &*hook.name)],
        text,
    );
}

/// Returns whether the hook returns `bool`. That is the case when it can
/// return or jump depending on its result.
fn hook_returns_bool(hook: &MidAsmHook) -> bool {
    hook.return_on_false
        || hook.return_on_true
        || hook.jump_address_on_false != 0
        || hook.jump_address_on_true != 0
}

/// Adds what runs after the hook: `return;`, a jump, or nothing.
fn push_after_hook(
    text: &mut String,
    ret: bool,
    jump_address: u32,
    indent: &str,
    is_local: &impl Fn(GuestAddress32) -> bool,
    debug: bool,
) {
    if ret {
        text.push_str(indent);
        text.push_str("return;\n");
    } else if jump_address != 0 {
        push_hook_transfer(text, jump_address.into(), indent, is_local, debug);
    }
}

/// Adds the call, like `test_hook(ctx, base)`.
fn push_call(text: &mut String, hook: &MidAsmHook) {
    text.push_str(&hook.name);
    text.push_str("(ctx, base)");
}

/// Adds a hook's `goto` to `address`, indented by `indent`.
fn push_hook_transfer(
    text: &mut String,
    address: GuestAddress32,
    indent: &str,
    is_local: &impl Fn(GuestAddress32) -> bool,
    debug: bool,
) {
    if !is_local(address) {
        if debug {
            text.push_str(indent);
            let _ = writeln!(
                text,
                "fprintf(stderr, \"midasm hook jumps outside function: 0x{address:X}\\n\");"
            );
            text.push_str(indent);
            text.push_str("fflush(stderr);\n");
        }
        text.push_str(indent);
        text.push_str("std::abort();\n");
        return;
    }

    text.push_str(indent);
    text.push_str("goto loc_");
    let _ = write!(text, "{address:X}");
    text.push_str(";\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::rstest;

    #[rstest]
    #[case::local(0x1000, false, "goto loc_1000;\n")]
    #[case::missing_label(0x1001, false, "std::abort();\n")]
    #[case::outside(0x1002, false, "std::abort();\n")]
    #[case::debug_outside(
        0x1002,
        true,
        "fprintf(stderr, \"midasm hook jumps outside function: 0x1002\\n\");\n\tfflush(stderr);\n\tstd::abort();\n"
    )]
    fn hook_should_check_labels_before_jump(
        #[case] target: u32,
        #[case] debug: bool,
        #[case] transfer: &str,
    ) {
        // Arrange.
        let mut config = RecompilerConfig {
            debug,
            ..RecompilerConfig::default()
        };
        config.midasm_hooks.insert_or_overwrite(
            0x1000.into(),
            MidAsmHook {
                name: "test_hook".into(),
                jump_address: target,
                ..MidAsmHook::default()
            },
        );
        let mut text = String::new();

        // Act.
        write_hook_call(
            &config,
            0x1000.into(),
            &LocalTargets::new([0x1000.into()]),
            None,
            &mut text,
        );

        // Assert.
        assert_eq!(text, format!("\ttest_hook(ctx, base);\n\t{transfer}"));
    }

    #[rstest]
    #[case::void(
        false,
        indoc! {"
            extern void test_hook(X86Context& __restrict ctx, uint8_t* base);

        "},
        "\ttest_hook(ctx, base);\n"
    )]
    #[case::conditional(
        true,
        indoc! {"
            extern bool test_hook(X86Context& __restrict ctx, uint8_t* base);

        "},
        "\tif (test_hook(ctx, base)) {\n\t}\n\telse {\n\t\treturn;\n\t}\n"
    )]
    fn hook_should_pass_context_and_return_type(
        #[case] return_on_false: bool,
        #[case] declaration: &str,
        #[case] call: &str,
    ) {
        // Arrange
        let mut config = RecompilerConfig::default();
        config.midasm_hooks.insert_or_overwrite(
            0x1000.into(),
            MidAsmHook {
                name: "test_hook".into(),
                return_on_false,
                ..MidAsmHook::default()
            },
        );
        let mut declaration_text = String::new();
        let mut call_text = String::new();

        // Act
        write_hook_declaration(&config, 0x1000.into(), &mut declaration_text);
        write_hook_call(
            &config,
            0x1000.into(),
            &LocalTargets::default(),
            None,
            &mut call_text,
        );

        // Assert
        assert_eq!(declaration_text, declaration);
        assert_eq!(call_text, call);
    }
}
