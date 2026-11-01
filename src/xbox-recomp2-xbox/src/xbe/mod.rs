//! Loads XBE files (Xbox executables) into the format-neutral [`Executable`].
//!
//! Reference: <https://xbdev.net/fileformats/xbe/NonOfficialXBEFileFormat.pdf>

use crate::xbe::header::{XbeHeader, XbeLibraryVersion, XbeSectionFlags, XbeSectionHeader};
use endian_writer::{EndianReader, HasSize, LittleEndianReader};
use xbox_recomp2_executable::{ConfigDefine, Executable, Section, SectionFlags, read_c_string};

mod decrypt;
pub mod header;
mod kernel;
#[cfg(test)]
mod test_helpers;

/// Loads an XBE file into an [`Executable`].
///
/// # Arguments
///
/// * `data` - The whole XBE file.
///
/// # Returns
///
/// The parsed executable.
///
/// # Panics
///
/// - When `data` doesn't start with the `XBEH` magic.
/// - When the section header table or library version are outside the file.
///
/// # Safety
///
/// Small risk of code going kaboom down the road if `data` points to a
/// malformed XBE. You know, unsafe code and all.
///
/// [`ExecutableFormat::detect`]: xbox_recomp2_executable::ExecutableFormat::detect
pub(super) unsafe fn load_executable(data: &[u8]) -> Executable {
    let mut executable = Executable {
        size: data.len() as u32,
        data: data.to_vec(),
        // Addresses from 0x80000000 up belong to the Xbox kernel.
        address_limit: Some(0x8000_0000u64.into()),
        ..Default::default()
    };

    // SAFETY: data isn't resized while in use
    let mut reader = unsafe { LittleEndianReader::new(executable.data.as_ptr()) };

    // SAFETY: the caller guarantees the header fits in `data`.
    let header = unsafe { reader.read_at::<XbeHeader>(0) };
    assert_eq!(
        header.magic, *b"XBEH",
        "XBE loader reached with non-XBE input, grand dad, fleenstones!?"
    );

    executable.entry_point = decrypt::entry_point(header.entry, executable.size).into();
    executable.size_of_image = header.size_of_image as u64;
    executable.base = header.base_address.into();

    // We have faith in the user to pass a valid XBE.
    // But if they didn't kaboom!
    let headers_offset = header.section_headers_address.to_file_offset(&header);
    assert!(
        headers_offset < executable.size,
        "XBE section-headers offset {headers_offset:#x} falls outside the {}-byte file",
        executable.size
    );

    // A zero address means the file lists no libraries.
    let d3d8_version = if header.library_versions_addr.is_zero() {
        0
    } else {
        find_d3d8_build_version(&executable.data, &header)
    };
    executable.config_defines.push(ConfigDefine {
        name: "X86_CODE_D3D8V",
        value: u64::from(d3d8_version),
    });

    // SAFETY: `headers_offset` is inside the file (checked above).
    unsafe { reader.seek(headers_offset as isize) };

    // Okay let's parse our sections.
    let mut sections = Vec::new();
    for _ in 0..header.num_sections {
        let section = unsafe { reader.read::<XbeSectionHeader>() };

        // Discard if no data is in the file
        if !section_fits(&executable, &header, section) {
            continue;
        }

        let name_offset = section.section_name_address.to_file_offset(&header);
        let name = read_c_string(&executable.data, name_offset as usize);

        let is_executable = section.flags.contains(XbeSectionFlags::EXECUTABLE);
        sections.push(Section {
            raw_size: section.raw_size,
            ..Section::new(
                &name,
                section.virtual_address,
                section.virtual_size,
                section_flags(&name, is_executable),
                section.raw_address,
            )
        });
    }

    // Later code looks sections up by address, so sort them and keep
    // only the first section at each address.
    sections.sort_by_key(|section| section.base);
    sections.dedup_by_key(|section| section.base);
    executable.sections = sections;

    // Decrypt the kernel thunk (import) table entry.
    let kernel_thunk_table =
        decrypt::kernel_thunk_addr(header.kernel_image_thunk_addr, executable.size);
    kernel::load_kernel_functions(&mut executable, kernel_thunk_table);

    executable
}

/// Finds the D3D8 build number.
///
/// # Returns
///
/// The build number of the first `D3D8` entry.
///
/// # Remarks
///
/// This is kinda important cause different D3D8 versions pass data
/// in different formats. When doing a recomp you'll want to know that.
///
/// # Panics
///
/// When the table ends past the end of `data`.
/// We trust files that pass the magic check, so this means a loader bug.
fn find_d3d8_build_version(data: &[u8], header: &XbeHeader) -> u32 {
    let library_offset = header.library_versions_addr.to_file_offset(header);

    let entry_size = XbeLibraryVersion::SIZE as u64;
    let table_end = u64::from(library_offset) + u64::from(header.library_versions) * entry_size;
    assert!(
        table_end <= data.len() as u64,
        "XBE library-version table end {table_end:#x} falls outside the {}-byte file",
        data.len()
    );

    // SAFETY: the whole table fits in `data` (checked above).
    let mut reader = unsafe { LittleEndianReader::new(data.as_ptr().add(library_offset as usize)) };

    (0..header.library_versions)
        // SAFETY: as above.
        .map(|_| unsafe { reader.read::<XbeLibraryVersion>() })
        .find(|library| library.name() == b"D3D8")
        .map_or(0, |library| u32::from(library.build_version))
}

/// Returns `true` when the section's bytes lie inside the file..
fn section_fits(executable: &Executable, header: &XbeHeader, section: XbeSectionHeader) -> bool {
    let in_file = section.raw_address < executable.size
        && section.raw_address.wrapping_add(section.raw_size) <= executable.size;

    let in_image = section.virtual_address >= header.base_address
        && section.virtual_address.wrapping_add(section.virtual_size)
            <= header.base_address.wrapping_add(header.size_of_image);

    in_file && in_image
}

/// Marks a section as code or data.
///
/// # Remarks
///
/// See [`header::is_data_section`] and `DATA_SECTION_NAMES`.
///
/// Many Xbox binaries don't set the executable flag correctly.
/// You'll want to know some things.
fn section_flags(name: &str, is_executable: bool) -> SectionFlags {
    if is_executable && !header::is_data_section(name) {
        SectionFlags::CODE
    } else {
        SectionFlags::DATA
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use core::mem::offset_of;
    use rstest::rstest;

    /// Reads the D3D8 build number the loader stored as a config define.
    fn d3d8_version(executable: &Executable) -> u64 {
        executable
            .config_defines
            .iter()
            .find(|define| define.name == "X86_CODE_D3D8V")
            .expect("every XBE load adds the D3D8 define")
            .value
    }

    /// Lists section names in the order the loader kept them.
    fn section_names(executable: &Executable) -> Vec<&str> {
        executable
            .sections
            .iter()
            .map(|section| section.name.as_str())
            .collect()
    }

    #[test]
    fn load_should_map_a_basic_xbe() {
        let executable = load(&build_xbe(&[]));

        assert_eq!(executable.base.get(), u64::from(BASE));
        assert_eq!(executable.entry_point.get(), u64::from(TEXT));
        assert_eq!(executable.size_of_image, u64::from(SIZE_OF_IMAGE));

        assert_eq!(section_names(&executable), [".text", ".rdata"]);
        let text = &executable.sections[0];
        assert_eq!(text.flags, SectionFlags::CODE);
        // Memory size and stored size differ, so a mix-up shows here.
        assert_eq!((text.size, text.raw_size), (0x800, 0x400));
        assert_eq!(executable.sections[1].flags, SectionFlags::DATA);
    }

    #[rstest]
    #[case::sorted_by_address(TEXT_HEADER, RDATA + 0x1000, &[".rdata", ".text"])]
    #[case::first_of_equal_addresses_wins(RDATA_HEADER, TEXT, &[".text"])]
    fn sections_should_be_sorted_and_deduplicated(
        #[case] header: usize,
        #[case] address: u32,
        #[case] expected: &[&str],
    ) {
        let mut data = build_xbe(&[]);
        let field = offset_of!(XbeSectionHeader, virtual_address);
        write_u32(&mut data, header + field, address);

        let executable = load(&data);

        assert_eq!(section_names(&executable), expected);
    }

    #[rstest]
    #[case::past_file_end(offset_of!(XbeSectionHeader, raw_address))]
    #[case::past_image_end(offset_of!(XbeSectionHeader, virtual_address))]
    fn sections_should_be_skipped_when_out_of_bounds(#[case] field: usize) {
        let mut data = build_xbe(&[]);
        write_u32(&mut data, TEXT_HEADER + field, 0x8000);

        let executable = load(&data);

        assert_eq!(section_names(&executable), [".rdata"]);
    }

    /// Many XBEs mark data sections as executable; the name wins.
    #[test]
    fn section_flags_should_keep_executable_data_sections_as_data() {
        // See [`header::is_data_section`] and `DATA_SECTION_NAMES` for context.
        assert_eq!(section_flags(".data", true), SectionFlags::DATA);
    }

    /// Games could list their libraries in any order.
    #[rstest]
    #[case::not_first_in_table(&[("XAPILIB", 1111), ("D3D8", 4054)], 4054)]
    fn d3d8_version_should_match_by_name(#[case] libraries: &[(&str, u16)], #[case] expected: u64) {
        let mut data = build_xbe(&[]);
        set_libraries(&mut data, libraries);

        let executable = load(&data);

        assert_eq!(d3d8_version(&executable), expected);
    }
}
