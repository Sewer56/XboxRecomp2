//! Checks that each template renders exactly its saved fixture file.
//!
//! Keep the fake inputs here in sync with the fixtures.

use super::*;

/// Embeds a fixture file at compile time.
macro_rules! golden {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/goldens/",
            $name
        ))
    };
}

const CONTEXT_FIXTURE_BODY: &str = concat!(
    "struct CtxFixture { int field; };\n",
    "static_assert(sizeof(CtxFixture) == 4, \"fixture\");\n",
);
/// Fake symbols as `(name, address, override allowed)`, in output order.
const SYMBOLS: [(&str, u64, u8); 4] = [
    ("FuncOne", 0x1100, 0),
    ("func_two", 0x2345, 1),
    ("__xbox_entry_point", 0x3333, 0),
    ("__imp__DbgPrint", 0x9000, 1),
];

// ── Captured text ────────────────────────────────────────────────────────────

/// The four generated header and table files match their fixtures:
///
/// - `x86_config.h`;
/// - `x86_context.h`, with a header body after the fixed start;
/// - `x86_recomp_shared.h`, with one `extern` line per symbol;
/// - `x86_func_mapping.cpp`, with one entry per symbol.
#[test]
fn file_headers_match_captured_text() {
    let owned_args = config_header_args();
    let args: Vec<(&str, &str)> = owned_args
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    assert_eq!(render(CONFIG_HEADER, &args), golden!("config_header.txt"));

    let mut context = render(CONTEXT_HEADER_PREFIX, &[]);
    context.push_str(CONTEXT_FIXTURE_BODY);
    assert_eq!(context, golden!("context_header.txt"));

    let mut shared = render(SHARED_HEADER_HEAD, &[]);
    shared.push_str(&extern_entries());
    assert_eq!(shared, golden!("shared_header.txt"));

    let mut mapping = render(FUNC_MAPPING_HEAD, &[]);
    mapping.push_str(&mapping_entries());
    mapping.push_str(&render(FUNC_MAPPING_TAIL, &[]));
    assert_eq!(mapping, golden!("func_mapping.txt"));
}

/// The text around each function matches its fixture:
///
/// - weak and protected opening lines;
/// - the closing brace and blank line;
/// - the `sub_<address>` name.
#[test]
fn function_boundaries_match_captured_text() {
    let weak = render(
        FUNC_PROLOGUE,
        &[("name", "FuncOne"), ("linkage", "X86_WEAK_FUNC")],
    );
    assert_eq!(weak, golden!("function_prologue_weak.txt"));

    assert_eq!(
        render(
            FUNC_PROLOGUE,
            &[("name", "func_two"), ("linkage", "X86_PROTECTED_FUNC")]
        ),
        golden!("function_prologue_protected.txt")
    );

    assert_eq!(
        render(FUNC_BODY_CLOSE, &[]),
        golden!("function_body_close.txt")
    );

    assert_eq!(
        render(SUB_NAME, &[("address", "4ABC")]),
        golden!("sub_name.txt")
    );
}

/// Lines inside a function match their fixture:
///
/// - labels;
/// - the three scratch variable declarations, each followed by a blank line;
/// - "missing instruction" lines (uppercase address);
/// - "doesn't end with JMP or RET" lines (lowercase address).
#[test]
fn in_function_lines_match_captured_text() {
    assert_eq!(render(LABEL, &[("address", "1104")]), golden!("label.txt"));

    let render_temps = |template: &str, count: u64| {
        let mut temps = String::new();
        for index in 0..count {
            render_into(template, &[("index", &format!("{index}"))], &mut temps);
        }
        temps
    };

    assert_eq!(render_temps(TEMP_UINT64, 2), golden!("temp_uint64.txt"));
    assert_eq!(render_temps(TEMP_UINT32, 2), golden!("temp_uint32.txt"));
    assert_eq!(render_temps(TEMP_DOUBLE, 3), golden!("temp_double.txt"));

    let mut missing = render(MISSING_INSTRUCTION_COMMENT, &[]);
    missing.push_str(&render(
        MISSING_INSTRUCTION_PRINTF,
        &[("disasm", "mov eax, ebx"), ("address", "12AB")],
    ));
    assert_eq!(missing, golden!("missing_instruction.txt"));

    let mut warning = render(LAST_INST_WARNING_PRINTF, &[("address", "cd00")]);
    warning.push_str(&render(LAST_INST_WARNING_COMMENT, &[]));
    assert_eq!(warning, golden!("last_inst_warning.txt"));
}

/// A hook declaration matches its fixture.
#[test]
fn midasm_hook_decl_matches_captured_text() {
    assert_eq!(
        render(
            MIDASM_HOOK_DECL,
            &[("returns", "void"), ("name", "mid_hook")],
        ),
        golden!("midasm_hook_decl.txt")
    );
}

// ── Edge cases ───────────────────────────────────────────────────────────────

/// A placeholder with no value panics, naming the placeholder.
#[test]
#[should_panic(expected = "no value for placeholder \"address\"")]
fn render_panics_on_missing_placeholder_value() {
    render(LABEL, &[]);
}

// ── Split include ────────────────────────────────────────────────────────────

/// The `#include` line at the top of each `x86_recomp.<index>.cpp`
/// matches its fixture.
#[test]
fn split_include_matches_captured_text() {
    assert_eq!(
        render(RECOMP_SPLIT_INCLUDE, &[]),
        golden!("split_include.txt")
    );
}

/// Config header values for the fake executable.
fn config_header_args() -> Vec<(&'static str, String)> {
    vec![
        ("image_base", "10000".to_string()),
        ("image_size", "8D3000".to_string()),
        (
            "extra_defines",
            "#define X86_CODE_D3D8V 5849ull\n".to_string(),
        ),
        ("code_base", "1000".to_string()),
        ("code_size", "6E00".to_string()),
    ]
}

fn extern_entries() -> String {
    let mut entries = String::new();
    for (name, address, allowed) in SYMBOLS {
        entries.push_str(&format!("#define X86_OVERRIDE_ALLOWED_{name} {allowed}\n"));
        entries.push_str(&format!(
            "#define X86_OVERRIDE_ADDRESS_{name} \"0x{address:X}\"\n"
        ));
        render_into(EXTERN_FUNC_ENTRY, &[("name", name)], &mut entries);
    }
    entries
}

fn mapping_entries() -> String {
    let mut entries = String::new();
    for (name, address, _) in SYMBOLS {
        render_into(
            FUNC_MAPPING_ENTRY,
            &[("address", &format!("{address:X}")), ("name", name)],
            &mut entries,
        );
    }
    entries
}
