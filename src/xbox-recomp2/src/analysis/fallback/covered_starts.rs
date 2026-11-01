//! Track addresses the padding scan must skip.

use crate::analysis::graph::DiscoveryGraph;
use crate::config::RecompilerConfig;
use xbox_recomp2_executable::GuestAddress32;

/// The covered ranges and gaps ignored by the padding scan:
///
/// ```text
/// Stored ranges: [{ start: 0x1000, end: 0x1005 },
///                 { start: 0x1020, end: 0x1025 }]
/// Gap:           0x1005..0x1020 (not stored)
/// ```
pub(super) struct CoveredStarts {
    /// Sorted, with exclusive ends.
    ranges: Vec<CoveredRange>,
}

/// A covered byte range with an inclusive start and an exclusive end.
struct CoveredRange {
    start: u64,
    end: u64, // so we can go over `u32::MAX`
}

impl CoveredStarts {
    /// Builds the byte ranges that the padding scan must skip.
    ///
    /// Includes:
    /// - The bytes of each decoded instruction in `graph`.
    /// - One byte at each known function entry, even when its body is empty.
    /// - The function ranges in `config`, excluding zero-sized ranges.
    ///
    /// Merges overlapping or touching ranges while leaving gaps uncovered.
    pub(super) fn new(graph: &DiscoveryGraph, config: &RecompilerConfig) -> Self {
        let mut ranges: Vec<CoveredRange> = Vec::new();

        // Go over each function, and the instructions inside.

        // Instructions are sorted by address; so we're always ascending.
        for function in graph.functions.values() {
            let first_range = ranges.len();
            for instruction in function.instructions.values() {
                let start = u64::from(instruction.address.get());
                let end = instruction.end();

                // See if we can merge with last range.
                if ranges.len() > first_range // is there a previous range to consider?
                    && let Some(previous) = ranges.last_mut()
                    && start <= previous.end
                {
                    previous.end = previous.end.max(end);
                } else {
                    // Make a new range.
                    ranges.push(CoveredRange { start, end });
                }
            }

            // Cover the entry even with no instructions.
            let entry = u64::from(function.entry.get());
            ranges.push(CoveredRange {
                start: entry,
                end: entry + 1,
            });
        }

        // Add the configured function ranges.
        ranges.extend(config.functions.iter().map(|(start, size)| {
            let start = u64::from(start.0);

            CoveredRange {
                start,
                end: start + u64::from(size.0),
            }
        }));

        // Empty configured ranges cover no bytes.
        ranges.retain(|range| range.start < range.end);
        ranges.sort_unstable_by_key(|range| (range.start, range.end));

        // We now have ranges per function.

        // But functions can be joined end-to-end when no padding happens.
        // So combine overlapping/touching ranges for optimisation.
        let mut merged: Vec<CoveredRange> = Vec::new();

        for range in ranges {
            if let Some(previous) = merged.last_mut()
                // ranges can OVERLAP, due to false positive entries
                // (where one function ends inside another, possibly).
                // We need to factor this in, hence <=
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }

        Self { ranges: merged }
    }

    /// Checks whether a stored range covers `address`.
    ///
    /// Range starts are included; ends are excluded.
    pub(super) fn contains(&self, address: GuestAddress32) -> bool {
        let address = u64::from(address.get());

        // Find the first range ending after the address.
        let index = self.ranges.partition_point(|range| range.end <= address);

        // Then check its start...
        self.ranges
            .get(index)
            .is_some_and(|range| range.start <= address)
    }
}

#[cfg(test)]
mod tests {
    //! Check known entries and configured ranges without decoding instructions.

    use super::*;
    use crate::analysis::graph::FunctionDiscoverySource;
    use crate::config::FunctionSize;
    use rstest::rstest;

    #[rstest]
    #[case::before(0x0fff, false)]
    #[case::entry_without_body(0x1000, true)]
    #[case::after_entry(0x1001, false)]
    #[case::manual_start(0x1010, true)]
    #[case::overlap(0x101f, true)]
    #[case::adjacent(0x1030, true)]
    #[case::manual_end(0x1040, false)]
    #[case::empty_manual_range(0x1050, false)]
    #[case::high_address(u32::MAX, true)]
    fn contains_should_respect_entry_and_range_bounds(
        #[case] address: u32,
        #[case] expected: bool,
    ) {
        // Arrange.
        // Overlapping and touching ranges combine; empty ranges cover nothing.
        //
        // ```text
        //           0x1010 0x1018  0x1020        0x1030         0x1040
        // Range 1:  [-------------------------------)
        // Range 2:          [-------)
        // Range 3:                                  [---------------)
        // Merged:   [-----------------------------------------------)
        //
        // [ = included start; ) = excluded end
        // Empty: 0x1050..0x1050 covers no bytes.
        // ```
        let mut config = RecompilerConfig::default();
        config
            .functions
            .insert_or_overwrite(0x1010.into(), FunctionSize(0x20));
        config
            .functions
            .insert_or_overwrite(0x1018.into(), FunctionSize(0x08));
        config
            .functions
            .insert_or_overwrite(0x1030.into(), FunctionSize(0x10));
        config
            .functions
            .insert_or_overwrite(0x1050.into(), FunctionSize(0));
        config
            .functions
            .insert_or_overwrite((u32::MAX - 1).into(), FunctionSize(4));
        let mut graph = DiscoveryGraph::default();
        graph.add_function(
            GuestAddress32::from(0x1000),
            FunctionDiscoverySource::EntryPoint,
            None,
        );
        let covered = CoveredStarts::new(&graph, &config);

        // Act.
        let result = covered.contains(GuestAddress32::from(address));

        // Assert.
        assert_eq!(result, expected);
    }
}
