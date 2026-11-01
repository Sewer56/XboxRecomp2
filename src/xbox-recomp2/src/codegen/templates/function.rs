//! Templates for the text around each generated function.

/// Suffix for function where calling convention was changed for optimisation,
/// like `sub_401000_fastcall`.
pub const FASTCALL_SUFFIX: &str = "_fastcall";
/// Closes a function.
pub const FUNC_BODY_CLOSE: &str = r#"}

"#;
/// Opens a function.
pub const FUNC_PROLOGUE: &str = r#"{{linkage}}({{name}}) {
"#;
/// Jump target, such as `loc_401000:`.
pub const LABEL: &str = r#"loc_{{address}}:
"#;
/// Marks a function that doesn't end with `jmp` or `ret`.
pub const LAST_INST_WARNING_COMMENT: &str = r#"	//WARNING Function doesn't end with JMP or RET, possible analysis error.
"#;
/// Runtime version of [`LAST_INST_WARNING_COMMENT`]. Debug only.
///
/// `address` is lowercase hex.
pub const LAST_INST_WARNING_PRINTF: &str = r#"	printf("WARNING Function doesn't end with JMP or RET, possible analysis error. {{address}}\n");
"#;
/// Declares a mid-assembly hook.
///
/// `returns` is `void` or `bool`.
pub const MIDASM_HOOK_DECL: &str = r#"extern {{returns}} {{name}}(X86Context& __restrict ctx, uint8_t* base);

"#;
/// Stands in for an instruction the recompiler can't translate.
pub const MISSING_INSTRUCTION_COMMENT: &str = r#"	//MISSING INSTRUCTION HERE TODO ADD
"#;
/// Runtime version of [`MISSING_INSTRUCTION_COMMENT`]. Debug only.
pub const MISSING_INSTRUCTION_PRINTF: &str = r#"	printf("Missing instruction: {{disasm}} - {{address}}\n");
"#;
/// Name for a function without a symbol, such as `sub_401000`.
pub const SUB_NAME: &str = r#"sub_{{address}}"#;
/// Declares the `x87_float` scratch variable `val_double_N`.
pub const TEMP_DOUBLE: &str = r#"	x87_float val_double_{{index}} = 0;

"#;
/// Declares the `uint32_t` scratch variable `val_uint32_N`.
pub const TEMP_UINT32: &str = r#"	uint32_t val_uint32_{{index}} = 0;

"#;
/// Declares the `uint64_t` scratch variable `val_uint64_N`.
pub const TEMP_UINT64: &str = r#"	uint64_t val_uint64_{{index}} = 0;

"#;
