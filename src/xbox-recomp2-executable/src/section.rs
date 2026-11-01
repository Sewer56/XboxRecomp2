//! Executable sections and their access flags.

bitflags! {
    /// Section access flags, an 8-bit bitmask.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct SectionFlags: u8 {
        /// Section holds data.
        const DATA = 1;

        /// Section holds executable code.
        const CODE = 2;
    }
}

use super::address::VirtualAddress;
use bitflags::bitflags;

/// One mapped section of the guest executable, for example `.text`.
///
/// # Remarks
///
/// For people unfamiliar with executable file formats, there's an important
/// note that must be made (here or somewhere).
///
/// The section is `[base, base + size)` in guest (game's) virtual memory.
/// HOWEVER‼️, only `raw_size` bytes are actually stored in the file
/// (at `raw_address`).
///
/// Rest is ZERO filled at runtime.
///
/// Thank you for your attention to this matter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Section name (e.g. `.text`).
    pub name: String,

    /// Virtual base address of the section.
    /// (Where it starts in RAM inside game process)
    pub base: VirtualAddress<u64>,

    /// Virtual size of the section in bytes.
    /// (How much space it occupies in RAM inside game process)
    pub size: u32,

    /// Access flags.
    ///
    /// Meaningless on Xbox, and often **wrong!!**.
    /// <https://en.wikipedia.org/wiki/NX_bit>
    ///
    /// The 'non-execute' bit didn't exist on the Pentium-III
    /// based CPU.
    ///
    /// -----
    ///
    /// Also coincidentally the reason a lot of 2000s RenderWare PC games
    /// crash on modern OSes with DEP enabled. (They got it wrong too!)
    pub flags: SectionFlags,

    /// Location of raw data inside the executable file.
    pub raw_address: u32,

    /// Number of stored bytes in raw executable file.
    pub raw_size: u32,
}

impl Section {
    /// Creates a section where stored size equals its virtual size.
    ///
    /// # Arguments
    ///
    /// * `name` - section name as stored in the executable
    /// * `base` - virtual base address of the section
    /// * `size` - virtual size of the section in bytes
    /// * `flags` - access flags
    /// * `raw_address` - byte offset of the section's raw data inside
    ///   the executable
    pub fn new(
        name: &str,
        base: impl Into<VirtualAddress<u64>>,
        size: u32,
        flags: SectionFlags,
        raw_address: u32,
    ) -> Self {
        Self {
            name: name.to_owned(),
            base: base.into(),
            size,
            flags,
            raw_address,
            raw_size: size,
        }
    }

    /// Returns the section's raw bytes.
    ///
    /// - Reads stop at the smallest of the:
    ///     - Raw data (XBE/EXE) size
    ///     - Virtual (RAM) size
    ///     - Input length (array)
    ///
    /// # Arguments
    ///
    /// * `data` - the executable's complete file buffer.
    ///
    /// # Returns
    ///
    /// The available mapped bytes, or an empty slice if missing.
    pub fn file_bytes<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        let bytes = data.get(self.raw_address as usize..).unwrap_or(&[]);
        &bytes[..bytes.len().min(self.raw_size.min(self.size) as usize)]
    }

    /// The section's end address. (`base + size`)
    ///
    /// # Returns
    ///
    /// The first address past the section's bytes.
    pub fn end(&self) -> VirtualAddress<u64> {
        self.base.wrapping_add(self.size)
    }

    /// Returns `true` when `address` lies within `[base, base + size)`.
    ///
    /// # Arguments
    ///
    /// * `address` - the guest virtual address to test
    pub fn contains(&self, address: impl Into<VirtualAddress<u64>>) -> bool {
        let address = address.into();
        address >= self.base && address < self.end()
    }

    /// Translate guest RAM (memory) address to XBE/EXE file location.
    ///
    /// # Arguments
    ///
    /// * `address` - guest virtual (RAM) address to translate
    ///
    /// # Returns
    ///
    /// - `raw_address + (address - base)`: where the bytes at
    ///   `address` sit in the file.
    ///
    /// # Errors
    ///
    /// - [`None`] when this section doesn't contain `address`.
    /// - [`None`] when `address` is not executable backed (in zero-filled RAM).
    /// - [`None`] when the offset does not fit in `usize`.
    pub fn file_offset_of(&self, address: impl Into<VirtualAddress<u64>>) -> Option<usize> {
        let address = address.into();

        // Offset of address relative to the start of this section.
        let offset_into_section = address.wrapping_sub(self.base);

        // Only the first `raw_size` bytes exist in the file.
        // Rest is zero-filled at runtime when executable is mapped.
        // So it has no file offset.
        if !self.contains(address) || offset_into_section >= u64::from(self.raw_size) {
            return None;
        }

        // File offset of this section.
        // +
        // Offset of address relative to start of this section.
        usize::try_from(u64::from(self.raw_address) + offset_into_section).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── Core behaviour ───────────────────────────────────────────────────────

    /// The read ends at whichever size is smallest (raw, virtual, or file);
    /// each case makes a different one the winner.
    #[rstest]
    #[case::stored_end(2, 8, 8, &[2, 3])]
    #[case::virtual_end(8, 2, 8, &[2, 3])]
    #[case::file_end(8, 8, 4, &[2, 3])]
    #[case::empty_storage(0, 8, 8, &[])]
    #[case::missing_storage(8, 8, 1, &[])]
    fn file_bytes_should_stop_at_the_smallest_of_raw_virtual_or_file_size(
        #[case] raw_size: u32,
        #[case] virtual_size: u32,
        #[case] file_size: usize,
        #[case] expected: &[u8],
    ) {
        // Arrange
        // Note: The section starts at file offset 2, not at its guest base.
        let data = [0, 1, 2, 3, 4, 5, 6, 7];
        let section = Section {
            raw_size,
            ..Section::new(".text", 0x1000u64, virtual_size, SectionFlags::CODE, 2)
        };

        // Act.
        let bytes = section.file_bytes(&data[..file_size]);

        // Assert.
        assert_eq!(bytes, expected);
    }

    #[rstest]
    #[case::last_stored(0x1001, Some(3))]
    #[case::first_zero_filled(0x1002, None)]
    #[case::last_virtual(0x1007, None)]
    fn file_offset_of_should_exclude_zero_filled_virtual_memory(
        #[case] address: u64,
        #[case] expected: Option<usize>,
    ) {
        // Arrange.
        let section = Section {
            raw_size: 2,
            ..Section::new(".text", 0x1000u64, 8, SectionFlags::CODE, 2)
        };

        // Act.
        let offset = section.file_offset_of(address);

        // Assert
        assert!(section.contains(address));
        assert_eq!(section.end().get(), 0x1008);
        assert_eq!(offset, expected);
    }

    #[test]
    fn contains_includes_start_excludes_end() {
        let section = Section::new(".text", 0x1000u64, 0x100, SectionFlags::CODE, 0x200);
        assert!(section.contains(0x1000u64)); // start: included
        assert!(section.contains(0x10FFu64)); // last byte: included
        assert!(!section.contains(0x0FFFu64)); // byte before start: excluded
        assert!(!section.contains(0x1100u64)); // end: excluded
    }

    /// The section covers 0x1000 up to, but not including, 0x1100 in memory.
    /// Its bytes start at 0x200 in the file.
    /// Inside, file offset is 0x200 + distance from 0x1000.
    /// Outside, there is no file offset.
    #[rstest]
    #[case::before_start(0x0FFF, None)] // 1 byte before the section
    #[case::start(0x1000, Some(0x200))] // 0 bytes in
    #[case::middle(0x1004, Some(0x204))] // 4 bytes in
    #[case::last_byte(0x10FF, Some(0x2FF))] // 0xFF bytes in
    #[case::end(0x1100, None)] // first byte after the section
    #[case::past_end(0x2000, None)] // far past the section
    fn file_offset_of_maps_only_addresses_inside_the_section(
        #[case] address: u64,
        #[case] expected: Option<usize>,
    ) {
        let section = Section::new(".text", 0x1000u64, 0x100, SectionFlags::CODE, 0x200);
        assert_eq!(section.file_offset_of(address), expected);
    }
}
