//! Counters for the stats command.
//!
//! Used to help selelct stuff to optimize
//! (but truth is, I forgot about it lol)

use serde::Serialize;
use std::collections::BTreeMap;

/// Stage name for the switch table counters.
pub const SWITCH_TABLES_STAGE: &str = "switch-tables";

/// Counters, grouped by stage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CounterRegistry {
    /// Stage name to counter name to value.
    stages: BTreeMap<String, BTreeMap<String, u64>>,
}

impl CounterRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `value` to `counter` in `stage`.
    pub fn register(&mut self, stage: &str, counter: &str, value: u64) {
        *self
            .stages
            .entry(stage.to_owned())
            .or_default()
            .entry(counter.to_owned())
            .or_insert(0) += value;
    }

    /// Counters for `stage`, or `None` if it has none.
    pub fn stage(&self, stage: &str) -> Option<&BTreeMap<String, u64>> {
        self.stages.get(stage)
    }

    /// Every `(stage, counter, value)`, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, u64)> + '_ {
        self.stages.iter().flat_map(|(stage, counters)| {
            counters
                .iter()
                .map(move |(counter, value)| (stage.as_str(), counter.as_str(), *value))
        })
    }

    /// `true` if there are no counters.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Adds switch tables found in the executable.
    pub fn register_switch_tables(&mut self, discovered: usize, from_config: usize, merged: usize) {
        self.register(SWITCH_TABLES_STAGE, "discovered", discovered as u64);
        self.register(SWITCH_TABLES_STAGE, "from_config", from_config as u64);
        self.register(SWITCH_TABLES_STAGE, "merged", merged as u64);
    }
}
