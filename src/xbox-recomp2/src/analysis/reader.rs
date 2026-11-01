//! Read code and data without crossing mapping limits or excluded ranges.

use super::graph::StopReason;
use crate::config::RecompilerConfig;
use crate::executable::{Executable, SectionFlags};
use xbox_recomp2_executable::{GuestAddress32, GuestAddress64};

/// Reads executable bytes, skipping configured exclusions.
pub(super) struct ImageReader<'a> {
    pub(super) executable: &'a Executable,
    // Sorted, merged ranges: start included, end excluded.
    excluded: Vec<(u64, u64)>,
}

impl<'a> ImageReader<'a> {
    /// Builds a reader that skips the config's banned and invalid ranges.
    ///
    /// Merges touching or overlapping ranges, and ignores empty or reversed ones.
    ///
    /// # Arguments
    ///
    /// - `executable`: bytes to read.
    /// - `config`: ranges to skip.
    pub fn new(executable: &'a Executable, config: &RecompilerConfig) -> Self {
        let mut ranges =
            Vec::with_capacity(config.banned_addresses.len() + config.invalid_instructions.len());

        // Add banned address ranges
        for (start, end) in config.banned_addresses.iter() {
            if start.0 <= end.0 {
                let start = u64::from(start.0);
                let end = u64::from(end.0) + 1; // inclusive -> exclusive
                ranges.push((start, end));
            }
        }

        // Add invalid instructions
        for (start, size) in config.invalid_instructions.iter() {
            if size.0 != 0 {
                let start = u64::from(start.0);
                let end = start + u64::from(size.0);
                ranges.push((start, end));
            }
        }

        // Sort by address; we'll be joining them later.
        ranges.sort_unstable();

        // If the addresses are sussy bakas and touch or overlap, merge them.
        let mut excluded: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            // Get ze one from last loop.
            if let Some(previous) = excluded.last_mut()
                && start <= previous.1
            {
                // If start is at or before previous' end, merge the ranges.
                previous.1 = previous.1.max(end);
            } else {
                excluded.push((start, end));
            }
        }

        Self {
            executable,
            excluded,
        }
    }

    /// Checks whether `address` is in an exclusion.
    ///
    /// # Arguments
    ///
    /// - `address`: address to check.
    pub fn is_excluded(&self, address: GuestAddress32) -> bool {
        let address = u64::from(address.get());
        self.next_exclusion(address)
            .is_some_and(|&(start, _)| start <= address)
    }

    /// Skips the exclusion containing `address`.
    ///
    /// # Arguments
    ///
    /// - `address`: address where execution would continue.
    ///
    /// # Returns
    ///
    /// The address after the exclusion, or `address` if it isn't excluded.
    ///
    /// # Errors
    ///
    /// Returns [`StopReason::AddressOverflow`] if the next address cannot fit
    /// in [`u32`].
    pub fn skip_excluded(&self, address: GuestAddress32) -> Result<GuestAddress32, StopReason> {
        // Binary search the relevant range.
        let value = u64::from(address.get());
        match self.next_exclusion(value) {
            // basically
            // start <= value < end
            Some(&(start, end)) if start <= value => {
                GuestAddress32::try_from(GuestAddress64::from(end))
                    .map_err(|_| StopReason::AddressOverflow)
            }
            _ => Ok(address),
        }
    }

    /// Reads up to `maximum` bytes starting at `address`.
    ///
    /// Reads stop at the first of:
    ///
    /// - The section's stored or virtual end.
    /// - The file's end.
    /// - The next exclusion.
    /// - The configured address limit.
    ///
    /// # Arguments
    ///
    /// - `address`: first byte to read.
    /// - `flags`: flags the section must all have.
    /// - `maximum`: most bytes to return.
    ///
    /// # Returns
    ///
    /// A nonempty slice of file bytes.
    ///
    /// # Errors
    ///
    /// - [`StopReason::Unmapped`]: past the limit, outside a section or its stored
    ///   bytes, or `maximum` is zero.
    /// - [`StopReason::Excluded`]: `address` is inside an exclusion.
    /// - [`StopReason::NotExecutable`]: the section lacks a requested flag, even
    ///   for data reads.
    pub fn window(
        &self,
        address: GuestAddress32,
        flags: SectionFlags,
        maximum: usize,
    ) -> Result<&'a [u8], StopReason> {
        // Do not read past the address limit.
        let address = u64::from(address.get());
        let limit = self
            .executable
            .address_limit
            .map_or(1u64 << 32, |limit| limit.get().min(1u64 << 32));
        if address >= limit {
            return Err(StopReason::Unmapped);
        }

        // Stop inside an exclusion.
        let next = self.next_exclusion(address);
        if next.is_some_and(|&(start, _)| start <= address) {
            return Err(StopReason::Excluded);
        }

        // Read only from a section with the requested flags.
        let Some(section) = self.executable.section_containing(address) else {
            return Err(StopReason::Unmapped);
        };
        if !section.flags.contains(flags) {
            return Err(StopReason::NotExecutable);
        }

        // Read only bytes stored in the file.
        let offset = (address - section.base.get()) as usize;
        let bytes = section
            .file_bytes(&self.executable.data)
            .get(offset..)
            .unwrap_or(&[]);

        // Read at most `maximum` bytes without crossing a boundary.
        let end = limit.min(next.map_or(u64::MAX, |&(start, _)| start));
        let length = (end - address).min(maximum as u64).min(bytes.len() as u64) as usize;

        if length == 0 {
            return Err(StopReason::Unmapped);
        }

        Ok(&bytes[..length])
    }

    /// Reads up to 15 code bytes from `address`.
    ///
    /// That's enough for the longest x86 instruction. frfr, true story!
    ///
    /// # Arguments
    ///
    /// - `address`: first byte to read.
    ///
    /// # Returns
    ///
    /// A nonempty slice of code bytes.
    ///
    /// # Errors
    ///
    /// Same as [`ImageReader::window`] with [`SectionFlags::CODE`].
    pub fn code_window(&self, address: GuestAddress32) -> Result<&'a [u8], StopReason> {
        self.window(address, SectionFlags::CODE, 15)
    }

    /// Finds the exclusion containing `address`, or the next one.
    fn next_exclusion(&self, address: u64) -> Option<&(u64, u64)> {
        let index = self.excluded.partition_point(|&(_, end)| end <= address);
        self.excluded.get(index)
    }
}

#[cfg(test)]
mod tests {
    //! Test read limits and exclusions for code and data.

    use super::*;
    use crate::analysis::graph::test_helpers::{BASE, at, executable};
    use crate::config::{FunctionAddress, InvalidInstructionSize};
    use rstest::rstest;

    #[rstest]
    #[case::code(SectionFlags::CODE, 15, None, 15)]
    #[case::data(SectionFlags::DATA, usize::MAX, None, 32)]
    #[case::maximum(SectionFlags::DATA, 4, None, 4)]
    #[case::address_limit(SectionFlags::CODE, 15, Some(u64::from(BASE) + 3), 3)]
    #[case::wide_limit(SectionFlags::CODE, 15, Some(u64::MAX), 15)]
    fn window_should_respect_read_limits(
        #[case] flags: SectionFlags,
        #[case] maximum: usize,
        #[case] limit: Option<u64>,
        #[case] expected: usize,
    ) {
        // Arrange
        let mut executable = executable(&[0x90; 32]);
        executable.sections[0].flags = flags;
        executable.address_limit = limit.map(GuestAddress64::from);
        let reader = ImageReader::new(&executable, &RecompilerConfig::default());

        // Act
        let bytes = reader.window(at(0), flags, maximum).unwrap();

        // Assert
        assert_eq!(bytes.len(), expected);
    }

    #[rstest]
    #[case::virtual_end(4, 32, 32)]
    #[case::stored_end(32, 4, 32)]
    #[case::file_end(32, 32, 4)]
    fn window_should_stop_at_file_backed_bounds(
        #[case] virtual_size: u32,
        #[case] raw_size: u32,
        #[case] file_size: usize,
    ) {
        // Arrange
        let mut executable = executable(&[0x90; 32]);
        executable.sections[0].size = virtual_size;
        executable.sections[0].raw_size = raw_size;
        executable.data.truncate(file_size);
        let reader = ImageReader::new(&executable, &RecompilerConfig::default());

        // Act
        let bytes = reader
            .window(at(0), SectionFlags::CODE, usize::MAX)
            .unwrap();

        // Assert
        assert_eq!(bytes.len(), 4);
    }

    #[rstest]
    #[case::before(0, 0, Ok(2))]
    #[case::start(2, 9, Err(StopReason::Excluded))]
    #[case::overlap(4, 9, Err(StopReason::Excluded))]
    #[case::adjacent(8, 9, Err(StopReason::Excluded))]
    #[case::end(9, 9, Ok(15))]
    fn exclusions_should_merge_and_bound_reads(
        #[case] offset: u32,
        #[case] next: u32,
        #[case] expected: Result<usize, StopReason>,
    ) {
        // Arrange
        // Overlapping bans touch an invalid-instruction range at +7.
        let executable = executable(&[0x90; 32]);
        let mut config = RecompilerConfig::default();
        for (start, end) in [(2, 4), (4, 6), (12, 11)] {
            config
                .banned_addresses
                .insert_or_overwrite(FunctionAddress(BASE + start), FunctionAddress(BASE + end));
        }
        for (start, size) in [(7, 2), (10, 0)] {
            config
                .invalid_instructions
                .insert_or_overwrite(FunctionAddress(BASE + start), InvalidInstructionSize(size));
        }
        let reader = ImageReader::new(&executable, &config);

        // Act
        let window = reader.window(at(offset), SectionFlags::CODE, 15);
        let excluded = reader.is_excluded(at(offset));
        let skipped = reader.skip_excluded(at(offset));

        // Assert
        assert_eq!(excluded, expected.is_err());
        assert_eq!(skipped, Ok(at(next)));
        assert_eq!(window.map(|bytes| bytes.len()), expected);
    }

    #[rstest]
    #[case::space_end(u64::from(u32::MAX) + 1)]
    #[case::wide_limit(u64::MAX)]
    fn window_should_stop_at_32_bit_address_end(#[case] limit: u64) {
        // Arrange
        // The mapped section extends beyond the guest address space.
        let mut executable = executable(&[0x90; 32]);
        executable.sections[0].base = u64::from(u32::MAX - 1).into();
        executable.address_limit = Some(GuestAddress64::from(limit));
        let reader = ImageReader::new(&executable, &RecompilerConfig::default());

        // Act
        let window = reader.window(GuestAddress32::from(u32::MAX), SectionFlags::CODE, 15);

        // Assert
        assert_eq!(window.unwrap().len(), 1);
    }

    #[test]
    fn skip_excluded_should_report_address_overflow() {
        // Arrange
        let executable = executable(&[]);
        let mut config = RecompilerConfig::default();
        config
            .banned_addresses
            .insert_or_overwrite(FunctionAddress(u32::MAX), FunctionAddress(u32::MAX));
        let reader = ImageReader::new(&executable, &config);

        // Act
        let result = reader.skip_excluded(GuestAddress32::from(u32::MAX));

        // Assert
        assert_eq!(result, Err(StopReason::AddressOverflow));
    }
}
