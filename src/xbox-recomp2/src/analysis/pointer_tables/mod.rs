//! Find functions referenced by callback addresses and pointer tables.
//!
//! Used by both the code walk and the brute force scan.
//! Results are guesses: the guest may change a table before calling through it.
//!
//! See [function-discovery.md] for callback and pointer-table examples.
//!
//! [function-discovery.md]: https://github.com/Sewer56/XboxRecomp2/blob/main/doc/docs/function-discovery.md#4-discover-function-pointers-and-vtables

use crate::analysis::graph::{Flow, FunctionReference, Instruction};
use crate::analysis::reader::ImageReader;
use crate::config::RecompilerConfig;
use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder};
use crate::executable::{Executable, ThunkAddress};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

mod immediate;
mod indirect_calls;
mod tables;
#[cfg(test)]
pub(in crate::analysis) mod test_helpers;

/// Finds possible functions and remembers addresses it has already checked.
///
/// # Remarks
///
/// Use a new scanner if the file bytes or configured exclusions change.
pub(in crate::analysis) struct PointerScan<'a> {
    reader: ImageReader<'a>,
    decoder: &'a Decoder,
    buffer: DecodeBuffer,
    // Whether each checked address could start a function.
    callable: BTreeMap<GuestAddress32, bool>,
    // Targets found in each table.
    // Key = (Start Address, Entry Limit), Value = Vec of found targets.
    tables: BTreeMap<(GuestAddress32, Option<u32>), Vec<GuestAddress32>>,
}

/// Function references found during a pointer scan.
#[derive(Debug, Default)]
pub(in crate::analysis) struct Resolution {
    /// Found references. May contain duplicates.
    pub references: Vec<FunctionReference>,
}

impl<'a> PointerScan<'a> {
    /// Starts a scan with no addresses checked yet.
    ///
    /// # Arguments
    ///
    /// - `executable`: sections, file bytes, and imports to scan.
    /// - `config`: addresses to exclude from the scan. The reader copies these.
    /// - `decoder`: decoder used by the discovery pass.
    pub fn new(
        executable: &'a Executable,
        config: &RecompilerConfig,
        decoder: &'a Decoder,
    ) -> Self {
        Self {
            reader: ImageReader::new(executable, config),
            decoder,
            buffer: DecodeBuffer::new(),
            callable: BTreeMap::new(),
            tables: BTreeMap::new(),
        }
    }

    /// Reads code bytes so we can decode an instruction at `address`.
    ///
    /// # Arguments
    ///
    /// - `address`: guest address where the instruction would start.
    ///
    /// # Returns
    ///
    /// Up to 15 bytes, enough for the longest x86 instruction. frfr, true
    /// story!
    ///
    /// An empty slice means there are no readable code bytes at this address.
    ///
    /// # Remarks
    ///
    /// Stops early at section or file ends, exclusions, or the address limit.
    pub fn code_window(&self, address: GuestAddress32) -> &'a [u8] {
        self.reader.code_window(address).unwrap_or(&[])
    }

    /// Checks whether the code at `target` could be the start of a function.
    ///
    /// # Arguments
    ///
    /// - `target`: possible function entry. It does not need to be aligned.
    ///
    /// # Remarks
    ///
    /// Only checks the first instruction.
    /// Rejects any code which does not proceed to next line of execution.
    pub fn is_callable(&mut self, target: GuestAddress32) -> bool {
        // Cache check
        if let Some(&callable) = self.callable.get(&target) {
            return callable;
        }

        // Decode
        let window = self.code_window(target);
        let callable = !self.is_import(target)
            && match self.buffer.decode_at(self.decoder, window, 0) {
                // If it doesn't go to next line, it's not callable.
                DecodeOutcome::Ok(insn) => !matches!(
                    Instruction::from_decoded(target, insn).flow,
                    Flow::Stop | Flow::Unsupported
                ),
                DecodeOutcome::Failed { .. } => false,
            };
        self.callable.insert(target, callable);

        callable
    }

    /// Looks for function addresses used by one instruction.
    ///
    /// These instructions can reveal functions:
    /// - `push address`: pass a callback or pointer-table address.
    /// - `mov register, address` or `mov [object], address`: load or store one.
    /// - `call [address]`: read one function pointer from a data slot.
    /// - `call [index*4 + address]`: scan a table of four-byte function pointers.
    ///
    /// # Arguments
    ///
    /// - `instruction`: decoded instruction to inspect for pointer operands.
    /// - `result`: receives any references found.
    pub fn instruction(&mut self, instruction: &Instruction, result: &mut Resolution) {
        // Reject
        if instruction.clobbers_all || instruction.operand_width != 32 {
            return;
        }

        if matches!(instruction.flow, Flow::Call | Flow::Jump) {
            self.scan_indirect_call(instruction, result);
            return;
        }
        self.scan_immediate(instruction, result);
    }

    fn is_import(&self, address: GuestAddress32) -> bool {
        let imports = &self.reader.executable.imports;
        let slot = ThunkAddress(address.get());
        imports.resolve_thunk(slot).is_some() || imports.is_data_import(slot)
    }
}
