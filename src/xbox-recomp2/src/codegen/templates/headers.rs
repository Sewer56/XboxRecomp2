//! Text for the generated header and table files.
//!
//! - `x86_config.h`.
//! - The start of `x86_context.h`.
//! - `x86_recomp_shared.h`, with one `extern` line per symbol.
//! - `x86_func_mapping.cpp`, with one table entry per symbol.
//! - The `#include` line at the top of each `x86_recomp.<index>.cpp`.

/// The whole `x86_config.h` file.
///
/// Placeholders:
/// - `image_base`, `image_size`, `code_base`, `code_size`: uppercase hex.
/// - `extra_defines`: `#define` lines, each ending in a newline.
pub const CONFIG_HEADER: &str = r#"#pragma once
#ifndef X86_CONFIG_H_INCLUDED
#define X86_CONFIG_H_INCLUDED


#define X86_IMAGE_BASE 0x{{image_base}}ull
#define X86_IMAGE_SIZE 0x{{image_size}}ull
{{extra_defines}}#define X86_CODE_BASE 0x{{code_base}}ull
#define X86_CODE_SIZE 0x{{code_size}}ull

#ifdef X86_INCLUDE_DETAIL
#include "x86_detail.h"
#endif

#endif
"#;
/// Start of `x86_context.h`. The context header text follows it.
pub const CONTEXT_HEADER_PREFIX: &str = r#"#pragma once
#include "x86_config.h"

"#;
/// `extern` line for one symbol in `x86_recomp_shared.h`.
///
/// Placeholder: `name` (symbol name).
pub const EXTERN_FUNC_ENTRY: &str = "X86_EXTERN_FUNC({{name}});\n";
/// Table entry for one symbol in `x86_func_mapping.cpp`.
///
/// Placeholders: `address` (uppercase hex), `name` (symbol name).
pub const FUNC_MAPPING_ENTRY: &str = "\t{ 0x{{address}}, {{name}} },\n";
/// Start of `x86_func_mapping.cpp`, up to the first table entry.
pub const FUNC_MAPPING_HEAD: &str = r#"#include "x86_recomp_shared.h"

X86FuncMapping X86FuncMappings[] = {
"#;
/// End of `x86_func_mapping.cpp`.
pub const FUNC_MAPPING_TAIL: &str = "\t{ 0, nullptr }\n};\n";
/// `#include` line at the top of each `x86_recomp.<index>.cpp`.
pub const RECOMP_SPLIT_INCLUDE: &str = "#include \"x86_recomp_shared.h\"\n\n";
/// Start of `x86_recomp_shared.h`.
/// One [`EXTERN_FUNC_ENTRY`] per symbol follows it.
pub const SHARED_HEADER_HEAD: &str = r#"#pragma once

#include "x86_config.h"
#include "x86_context.h"

"#;
