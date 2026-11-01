//! Output file sizes.

use crate::GeneratedFileSet;
use std::collections::BTreeMap;

/// Size of each output file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputSizes {
    /// File name to size in bytes.
    pub files: BTreeMap<String, u64>,
}

impl OutputSizes {
    /// Gets the size of each generated file.
    pub(super) fn from_generated_files(files: &GeneratedFileSet) -> Self {
        Self {
            files: files
                .iter()
                .map(|(name, bytes)| (name.to_owned(), bytes.len() as u64))
                .collect(),
        }
    }

    /// Total size of all files, in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.files.values().sum()
    }

    /// Number of `x86_recomp.N.cpp` files.
    pub fn unit_count(&self) -> u64 {
        self.files
            .keys()
            .filter_map(|name| name.strip_prefix("x86_recomp.")?.strip_suffix(".cpp"))
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .count() as u64
    }
}
