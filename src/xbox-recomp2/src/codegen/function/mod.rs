//! Turns one guest function into C++.
//!
//! Generation runs in three steps:
//!
//! 1. [`build_function`] reads the function's instructions into a
//!    [`FunctionModel`]: blocks of operations, with labels and exits.
//! 2. [`crate::codegen::optimizations`] optimises it.
//! 3. [`write_function`] writes it as C++.
//!
//! [`FunctionModel`]: model::FunctionModel

use super::run_state::RunState;
use crate::analysis::{Function, FunctionBody};
use crate::config::RecompilerConfig;
use crate::decode::{DISASM_BUFFER_SIZE, DecodeBuffer, Decoder, DisasmFormatter};
use crate::executable::Executable;
use crate::platform::GuestPlatform;
use crate::templates::{SUB_NAME, render};
use model::FunctionModel;
use std::borrow::Cow;
use std::io::Write;
use write::emit::CodeWriter;
pub(in crate::codegen) use write::supports_addressing;
use xbox_recomp2_executable::GuestAddress32;

mod build;
pub(crate) mod model;
mod write;

/// Buffers kept between functions, so each function doesn't allocate new ones.
pub(crate) struct FunctionScratch {
    /// Used to decode instructions while building the model.
    decode: DecodeBuffer,

    /// Collects the C++ that an instruction's emitter writes.
    code: CodeWriter,

    /// Holds one piece of text before it is copied to the output.
    text: String,

    /// Turns each instruction into text for its `// ...` comment line.
    formatter: DisasmFormatter,

    /// Bytes the formatter writes into.
    disasm: Vec<u8>,
}

/// Details about the current function that the writing code reads.
struct GenerationContext<'a> {
    executable: &'a Executable,
    config: &'a RecompilerConfig,
    platform: &'a dyn GuestPlatform,

    /// Address of the function's first byte.
    base: GuestAddress32,
}

impl FunctionScratch {
    /// Creates empty buffers.
    pub(crate) fn new() -> Self {
        Self {
            decode: DecodeBuffer::new(),
            code: CodeWriter::new(),
            text: String::with_capacity(4096),
            formatter: DisasmFormatter::new(),
            disasm: vec![0; DISASM_BUFFER_SIZE],
        }
    }
}

/// Reads one function's instructions into a [`FunctionModel`].
///
/// # Arguments
///
/// * `function` - the function's address and size.
/// * `body` - the instructions to use and how they connect.
///   [`None`] decodes the whole range of `function`.
/// * `diag` - receives warnings about hooks.
///
/// # Returns
///
/// The model, ready for the optimisation passes and [`write_function`].
pub(crate) fn build_function(
    executable: &Executable,
    config: &RecompilerConfig,
    decoder: &Decoder,
    function: &Function,
    body: Option<&FunctionBody>,
    scratch: &mut FunctionScratch,
    diag: &mut dyn Write,
) -> FunctionModel {
    build::build(
        executable,
        config,
        decoder,
        &mut scratch.decode,
        function,
        body,
        diag,
    )
}

/// Writes one function as C++ and appends it to `out`.
pub(crate) fn write_function(
    executable: &Executable,
    config: &RecompilerConfig,
    platform: &dyn GuestPlatform,
    model: &FunctionModel,
    scratch: &mut FunctionScratch,
    state: &mut RunState<'_>,
    out: &mut Vec<u8>,
) {
    // Use the symbol name at the function's address, or `sub_<address>`
    // when there is no symbol.
    let base = GuestAddress32::from(model.address.0);
    let name: Cow<'_, str> = match executable.symbols.find(u64::from(base)) {
        Some(symbol) => Cow::Borrowed(&*symbol.name),
        None => Cow::Owned(render(SUB_NAME, &[("address", &format!("{base:X}"))])),
    };

    // Write the function as C++.
    let context = GenerationContext {
        executable,
        config,
        platform,
        base,
    };
    write::write(&context, model, &name, scratch, state, out);
}
