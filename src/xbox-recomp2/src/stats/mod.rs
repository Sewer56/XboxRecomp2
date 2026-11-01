//! Recompile stats: function count, instruction counts, output sizes etc.

use crate::executable::ExecutableFormat;
use crate::platform::GuestPlatform;
use crate::{MemoryWriter, Recompiler, RunError};
use ahash::HashMap;
pub use counters::{CounterRegistry, SWITCH_TABLES_STAGE};
pub use sizes::OutputSizes;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use zydis::Mnemonic;

pub mod counters;
mod sizes;

/// Count of each instruction.
#[derive(Debug, Default)]
struct InstructionHistogram {
    counts: HashMap<Mnemonic, u64>,
}

/// Stats returned by [`collect`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatsReport {
    /// Number of functions found.
    pub functions: u64,

    /// Count of each instruction, by name.
    pub mnemonics: BTreeMap<String, u64>,

    /// Size of each output file.
    pub output: OutputSizes,

    /// Per-stage counters.
    pub counters: CounterRegistry,
}

impl InstructionHistogram {
    /// Counts one instruction.
    fn count(&mut self, mnemonic: Mnemonic) {
        *self.counts.entry(mnemonic).or_insert(0) += 1;
    }

    /// Counts sorted by instruction name.
    fn sorted(&self) -> BTreeMap<String, u64> {
        self.counts
            .iter()
            .map(|(mnemonic, count)| (mnemonic.to_string(), *count))
            .collect()
    }
}

impl StatsReport {
    /// Total number of instructions.
    pub fn instructions(&self) -> u64 {
        self.mnemonics.values().sum()
    }

    /// Writes the report to `out` as text.
    pub fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "functions: {}", self.functions)?;
        writeln!(out, "instructions: {}", self.instructions())?;

        writeln!(out, "mnemonics:")?;
        for (name, count) in &self.mnemonics {
            writeln!(out, "  {name}: {count}")?;
        }

        writeln!(out, "output:")?;
        writeln!(out, "  total_bytes: {}", self.output.total_bytes())?;
        writeln!(out, "  unit_count: {}", self.output.unit_count())?;
        writeln!(out, "  files:")?;
        for (name, bytes) in &self.output.files {
            writeln!(out, "    {name}: {bytes}")?;
        }

        writeln!(out, "counters:")?;
        for (stage, counter, value) in self.counters.iter() {
            writeln!(out, "  {stage}:{counter}: {value}")?;
        }

        Ok(())
    }

    /// Writes the report to `out` as JSON.
    pub fn write_json(&self, out: &mut dyn Write) -> io::Result<()> {
        let report = serde_json::json!({
            "functions": self.functions,
            "instructions": self.instructions(),
            "mnemonics": self.mnemonics,
            "output": {
                "total_bytes": self.output.total_bytes(),
                "unit_count": self.output.unit_count(),
                "files": self.output.files,
            },
            "counters": &self.counters,
        });

        serde_json::to_writer_pretty(&mut *out, &report)?;
        out.write_all(b"\n")?;
        Ok(())
    }
}

/// Recompiles the config file `input` in memory and returns its stats.
pub fn collect(
    input: &Path,
    formats: &[&dyn ExecutableFormat],
    platform: &dyn GuestPlatform,
    diag: &mut dyn Write,
) -> Result<StatsReport, RunError> {
    let mut recompiler = Recompiler::load_input(input, formats, platform, diag)?;

    recompiler.analyse_requiring_functions(input, diag)?;

    let mut writer = MemoryWriter::new();
    let mut histogram = InstructionHistogram::default();
    let mut count = |mnemonic: Mnemonic| histogram.count(mnemonic);

    recompiler
        .generate(diag, &mut writer, &mut count)
        .map_err(RunError::Out)?;

    let mut counters = CounterRegistry::new();
    counters.register_switch_tables(
        recompiler.switch_tables_discovered,
        recompiler.switch_tables_from_config,
        recompiler.config.switch_tables.len(),
    );

    Ok(StatsReport {
        functions: recompiler.functions.len() as u64,
        mnemonics: histogram.sorted(),
        output: OutputSizes::from_generated_files(&writer.finish()),
        counters,
    })
}
