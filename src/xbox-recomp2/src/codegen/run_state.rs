//! Reports and counters that every function in a run writes into.
//!
//! [`Recompiler::generate`] builds one [`RunState`] per function. Each one
//! borrows the same run-wide lists:
//!
//! - instructions we don't support yet
//! - functions that may have been analysed wrong
//! - a per-mnemonic counter
//! - the diagnostics output
//!
//! [`Recompiler::generate`]: crate::Recompiler::generate

use crate::WrongAnalyse;
use ahash::HashSet;
use std::io::Write;
use zydis::Mnemonic;

/// Run-wide reports that one function's generation writes into.
pub(crate) struct RunState<'a> {
    /// Mnemonics with no emitter, in the order they were first seen.
    pub inst_missing: &'a mut Vec<String>,

    /// Same entries as `inst_missing`, for fast duplicate checks.
    pub inst_missing_index: &'a mut HashSet<String>,

    /// Functions whose last instruction is not a `jmp`, `ret` or `int3`,
    /// in the order they were first seen.
    pub wrong_analyse: &'a mut Vec<WrongAnalyse>,

    /// Same entries as `wrong_analyse`, for fast duplicate checks.
    pub wrong_analyse_index: &'a mut HashSet<WrongAnalyse>,

    /// Called once with the mnemonic of every instruction that decodes.
    ///
    /// The statistics collector counts mnemonics with it. Normal
    /// recompilation passes a function that does nothing.
    pub mnemonics: &'a mut dyn FnMut(Mnemonic),

    /// Where warnings and errors are printed.
    pub diag: &'a mut dyn Write,
}
