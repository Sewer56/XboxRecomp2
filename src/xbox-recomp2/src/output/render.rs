//! Renders the headers and mapping tables.

use crate::config::{FunctionAddress, RecompilerConfig};
use crate::executable::{Executable, SectionFlags, ThunkAddress};
use crate::templates::{
    CONFIG_HEADER, CONTEXT_HEADER_PREFIX, EXTERN_FUNC_ENTRY, FUNC_MAPPING_ENTRY, FUNC_MAPPING_HEAD,
    FUNC_MAPPING_TAIL, RECOMP_SPLIT_INCLUDE, SHARED_HEADER_HEAD, render_into,
};
use core::fmt::Write as _;

/// Adds `x86_config.h` content to `out`.
pub(crate) fn render_config_header(executable: &Executable, out: &mut Vec<u8>) {
    let mut code_min = u64::MAX;
    let mut code_max = 0u64;

    for section in &executable.sections {
        if section.flags.contains(SectionFlags::CODE) {
            code_min = code_min.min(section.base.get());
            code_max = code_max.max(section.end().get());
        }
    }

    let mut extra_defines = String::new();
    for define in &executable.config_defines {
        let _ = writeln!(extra_defines, "#define {} {}ull", define.name, define.value);
    }

    let mut scratch = String::new();
    render_into(
        CONFIG_HEADER,
        &[
            ("image_base", &format!("{:X}", executable.base.get())),
            ("image_size", &format!("{:X}", executable.size_of_image)),
            ("extra_defines", &extra_defines),
            ("code_base", &format!("{code_min:X}")),
            (
                "code_size",
                &format!("{:X}", code_max.wrapping_sub(code_min)),
            ),
        ],
        &mut scratch,
    );
    out.extend_from_slice(scratch.as_bytes());
}

/// Adds `x86_context.h` content to `out`.
pub(crate) fn render_context_header(context: &[u8], out: &mut Vec<u8>) {
    let mut scratch = String::new();
    render_into(CONTEXT_HEADER_PREFIX, &[], &mut scratch);
    out.extend_from_slice(scratch.as_bytes());

    out.extend_from_slice(context);
}

/// Adds `x86_func_mapping.cpp` content to `out`.
pub(crate) fn render_func_mapping(executable: &Executable, out: &mut Vec<u8>) {
    let mut scratch = String::new();
    render_into(FUNC_MAPPING_HEAD, &[], &mut scratch);

    for symbol in executable.symbols.iter() {
        render_into(
            FUNC_MAPPING_ENTRY,
            &[
                ("address", &format!("{:X}", symbol.address.get())),
                ("name", &*symbol.name),
            ],
            &mut scratch,
        );
    }

    render_into(FUNC_MAPPING_TAIL, &[], &mut scratch);
    out.extend_from_slice(scratch.as_bytes());
}

/// Adds `x86_recomp_shared.h` content to `out`.
pub(crate) fn render_shared_header(
    executable: &Executable,
    config: &RecompilerConfig,
    out: &mut Vec<u8>,
) {
    let mut scratch = String::new();
    render_into(SHARED_HEADER_HEAD, &[], &mut scratch);

    for symbol in executable.symbols.iter() {
        let allowed = u32::try_from(symbol.address.get())
            .is_ok_and(|address| is_override_allowed(executable, config, address));
        let _ = writeln!(
            scratch,
            "#define X86_OVERRIDE_ALLOWED_{} {}",
            symbol.name,
            u8::from(allowed)
        );
        let _ = writeln!(
            scratch,
            "#define X86_OVERRIDE_ADDRESS_{} \"0x{:X}\"",
            symbol.name,
            symbol.address.get()
        );
        render_into(EXTERN_FUNC_ENTRY, &[("name", &*symbol.name)], &mut scratch);
    }

    out.extend_from_slice(scratch.as_bytes());
}

/// Adds the `#include` line that opens each `x86_recomp.<index>.cpp`.
pub(crate) fn render_split_include(out: &mut Vec<u8>) {
    let mut scratch = String::new();
    render_into(RECOMP_SPLIT_INCLUDE, &[], &mut scratch);
    out.extend_from_slice(scratch.as_bytes());
}

/// Whether the host may replace the symbol at `address` with `X86_OVERRIDE_FUNC`.
///
/// # Remarks
///
/// Import thunks are always allowed.
/// They have no recompiled body, so the host must provide them.
fn is_override_allowed(executable: &Executable, config: &RecompilerConfig, address: u32) -> bool {
    config.function_hooks.contains(&FunctionAddress(address))
        || executable
            .imports
            .resolve_thunk(ThunkAddress(address))
            .is_some()
}
