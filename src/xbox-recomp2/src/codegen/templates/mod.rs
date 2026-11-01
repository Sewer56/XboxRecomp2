//! Fixed C++ text that surrounds the generated code.
//!
//! Shared templates cover:
//!
//! - File headers.
//! - Entries in the function mapping table.
//! - Function openings and closings.
//! - Labels and scratch variable declarations.
//! - Warning lines printed by the generated code.

pub use function::{
    FASTCALL_SUFFIX, FUNC_BODY_CLOSE, FUNC_PROLOGUE, LABEL, LAST_INST_WARNING_COMMENT,
    LAST_INST_WARNING_PRINTF, MIDASM_HOOK_DECL, MISSING_INSTRUCTION_COMMENT,
    MISSING_INSTRUCTION_PRINTF, SUB_NAME, TEMP_DOUBLE, TEMP_UINT32, TEMP_UINT64,
};
pub use headers::{
    CONFIG_HEADER, CONTEXT_HEADER_PREFIX, EXTERN_FUNC_ENTRY, FUNC_MAPPING_ENTRY, FUNC_MAPPING_HEAD,
    FUNC_MAPPING_TAIL, RECOMP_SPLIT_INCLUDE, SHARED_HEADER_HEAD,
};

mod function;
mod headers;
#[cfg(test)]
mod tests;

/// Fills in a template's `{{name}}` placeholders and returns the result.
///
/// ```rust
/// use xbox_recomp2::templates::{SUB_NAME, render};
///
/// assert_eq!(render(SUB_NAME, &[("address", "82001000")]), "sub_82001000");
/// ```
///
/// # Arguments
///
/// - `template`: text containing `{{name}}` placeholders.
/// - `args`: `(name, value)` pairs. Each value replaces its placeholder
///   exactly as given.
///
/// # Returns
///
/// `template` with every placeholder filled in.
///
/// # Panics
///
/// Panics if a placeholder has no closing `}}`, or if `args` has no value
/// for it. Templates are fixed in the code, so either one is a bug.
pub fn render(template: &str, args: &[(&str, &str)]) -> String {
    // Values are short (addresses and counts), so a little extra room
    // is usually enough.
    let mut out = String::with_capacity(template.len() + 16);
    render_into(template, args, &mut out);
    out
}

/// Like [`render`], but adds the result to the end of `out`.
///
/// # Arguments
///
/// - `template`: text containing `{{name}}` placeholders.
/// - `args`: `(name, value)` pairs. Each value replaces its placeholder
///   exactly as given.
/// - `out`: where the filled-in text is added.
///
/// # Panics
///
/// Panics if a placeholder has no closing `}}`, or if `args` has no value
/// for it. Templates are fixed in the code, so either one is a bug.
pub fn render_into(template: &str, args: &[(&str, &str)], out: &mut String) {
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);

        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .unwrap_or_else(|| panic!("unclosed placeholder in template near {after:?}"));
        let name = &after[..end];
        let value = args
            .iter()
            .find(|(arg_name, _)| *arg_name == name)
            .unwrap_or_else(|| panic!("no value for placeholder {name:?}"))
            .1;

        out.push_str(value);
        rest = &after[end + 2..];
    }

    out.push_str(rest);
}
