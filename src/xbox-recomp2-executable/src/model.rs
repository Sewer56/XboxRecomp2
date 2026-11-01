//! The guest executable the pipeline works with.

use super::ImportMap;
use super::address::VirtualAddress;
use super::section::Section;
use super::symbol_table::SymbolTable;

/// A parsed guest executable.
///
/// Whole input file is stored in [`Executable::data`].
/// Its sections are in [`Executable::sections`].
///
/// To read the bytes at a guest RAM address, call [`Executable::find`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Executable {
    /// The raw file bytes.
    pub data: Vec<u8>,

    /// Guest virtual (RAM) address of the EXE/XBE's first byte.
    /// i.e. Base address
    ///
    /// Xbox games usually load at `0x10000`.
    /// Windows ones at 0x400000.
    pub base: VirtualAddress<u64>,

    /// Input file size in bytes.
    pub size: u32,

    /// Address of first instruction/entry point for the code.
    pub entry_point: VirtualAddress<u64>,

    /// Size of the executable in virtual memory (RAM).
    ///
    /// Commonly known as `SizeOfImage` in EXE/XBE headers.
    ///
    /// Basically offset of last byte from first one; including gaps
    /// between sections that [`Executable::size`] does not cover.
    pub size_of_image: u64,

    /// Addresses at or above this value never resolve in
    /// [`Executable::find`].
    ///
    /// Some platforms like the Xbox have split address space for
    /// user and kernel memory. A barrier of what game code is allowed
    /// to access.
    ///
    /// On Xbox that is `0x8000_0000`.
    pub address_limit: Option<VirtualAddress<u64>>,

    /// Extra `#define`s for the generated `x86_config.h`.
    pub config_defines: Vec<ConfigDefine>,

    /// Mapped sections, sorted by base address.
    pub sections: Vec<Section>,

    /// Symbols collected from:
    /// - Imported functions. (kernel thunks, import table, etc.)
    /// - Later analysis.
    pub symbols: SymbolTable,

    /// Guest import state for this executable.
    pub imports: ImportMap,
}

/// One `#define` line in `x86_config.h`, e.g. `#define X86_CODE_D3D8V 5849ull`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDefine {
    pub name: &'static str,
    pub value: u64,
}

impl Executable {
    /// Returns stored bytes at a guest address.
    ///
    /// # Arguments
    ///
    /// * `address` - guest virtual address to read from
    ///
    /// # Returns
    ///
    /// [`None`] for:
    ///
    /// - addresses >= [`Executable::address_limit`].
    /// - addresses outside `[base, base + size)`.
    /// - addresses outside any section.
    /// - addresses not backed by raw file bytes.
    pub fn find(&self, address: impl Into<VirtualAddress<u64>>) -> Option<&[u8]> {
        let address = address.into();

        if self.address_limit.is_some_and(|limit| address >= limit) {
            return None;
        }

        if address < self.base || address >= self.base.wrapping_add(self.size) {
            return None;
        }

        let section = self.section_containing(address)?;
        let offset = usize::try_from(address.wrapping_sub(section.base)).ok()?;
        let bytes = section.file_bytes(&self.data).get(offset..)?;
        if bytes.is_empty() {
            return None;
        }

        let length = self.address_limit.map_or(bytes.len(), |limit| {
            (bytes.len() as u64).min(limit.wrapping_sub(address)) as usize
        });
        Some(&bytes[..length])
    }

    /// Returns the section containing `address`.
    ///
    /// # Arguments
    ///
    /// * `address` - guest virtual address to locate
    ///
    /// # Returns
    ///
    /// - The section containing `address`.
    /// - [`None`] otherwise
    pub fn section_containing(&self, address: impl Into<VirtualAddress<u64>>) -> Option<&Section> {
        let address = address.into();
        self.sections
            .iter()
            .find(|section| section.contains(address))
    }

    /// Finds a section with given name.
    ///
    /// # Arguments
    ///
    /// * `name` - section name to search for, e.g. `.text`
    ///
    /// # Returns
    ///
    /// The first section named `name` in address order, or [`None`]
    /// when no section has that name.
    pub fn find_section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SectionFlags;
    use rstest::rstest;

    /// Sections can be shorter in file than in virtual memory.
    /// (In which case, rest of space is zero filled at runtime.)
    ///
    /// `find` stops if going past the files stored in the exe/xbe.
    #[rstest]
    #[case::last_stored(0x1001, Some(&[3][..]))]
    #[case::zero_filled(0x1002, None)]
    fn find_should_not_read_past_section(#[case] address: u64, #[case] expected: Option<&[u8]>) {
        // Arrange
        // 8 bytes in memory; file bytes 2..4 stored.
        let data = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let executable = Executable {
            base: 0x1000u64.into(),
            size: data.len() as u32,
            sections: vec![Section {
                raw_size: 2, // store only 2 bytes
                ..Section::new(
                    ".text",
                    0x1000u64, // base
                    8,         // size
                    SectionFlags::CODE,
                    2, // file offset
                )
            }],
            data,
            ..Default::default()
        };

        // Act
        let bytes = executable.find(address);

        // Assert
        assert_eq!(bytes, expected);
    }
}
