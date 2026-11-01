//! Builds a small fake XBE for the loader tests.
//!
//! - [`build_xbe`]: makes the XBE.
//! - [`set_libraries`] and [`write_u32`]: patch it.
//! - [`load`]: runs the loader on it.

use super::header::{XbeHeader, XbeLibraryVersion, XbeSectionFlags, XbeSectionHeader};
use super::load_executable;
use endian_writer::{EndianWritableAt, EndianWriter, HasSize, LittleEndianWriter};
use xbox_recomp2_executable::Executable;

/// Memory address where the XBE starts.
pub(super) const BASE: u32 = 0x1000;
/// XOR key that hides the entry point in debug XBEs.
const DEBUG_ENTRY_KEY: u32 = 0x9485_9D4B;
/// XOR key that hides the kernel thunk table address in debug XBEs.
const DEBUG_THUNK_KEY: u32 = 0xEFB1_F152;
/// Length of the file.
const FILE_SIZE: usize = 0x4000;
/// File offset of the library version table.
const LIBRARY_VERSIONS: usize = 0x1C0;
/// Address of `.rdata`. The kernel thunk table starts here.
pub(super) const RDATA: u32 = 0x3000;
/// File offset of the `.rdata` section header, right after `.text`'s.
pub(super) const RDATA_HEADER: usize = TEXT_HEADER + XbeSectionHeader::SIZE;
/// File offset of `.rdata`'s bytes.
const RDATA_RAW: u32 = 0x1800;
/// File offset of the section name strings, 0x10 bytes apart.
const SECTION_NAMES: usize = 0x270;
/// Bytes of memory the XBE takes once loaded.
pub(super) const SIZE_OF_IMAGE: u32 = 0x5000;
/// Address of `.text`, which is also the entry point.
pub(super) const TEXT: u32 = 0x2000;
/// File offset of `.text`'s bytes.
const TEXT_RAW: u32 = 0x1000;
/// File offset of the `.text` section header.
pub(super) const TEXT_HEADER: usize = 0x200;

/// A header struct that [`write`] can write.
///
/// # Safety
///
/// as above.
unsafe trait FileStruct: EndianWritableAt + HasSize {}

// SAFETY: the derived `write_at` stays within `SIZE` bytes.
unsafe impl FileStruct for XbeHeader {}

// SAFETY: as above.
unsafe impl FileStruct for XbeLibraryVersion {}

// SAFETY: as above.
unsafe impl FileStruct for XbeSectionHeader {}

/// Builds an XBE with a `.text` and an `.rdata` section.
///
/// - `.text` takes 0x800 bytes of memory but stores only 0x400 in the file.
/// - `.rdata` starts with `thunks`, the kernel thunk table.
/// - The only library listed is D3D8, build 4054.
pub(super) fn build_xbe(thunks: &[u32]) -> Vec<u8> {
    let mut data = vec![0u8; FILE_SIZE];

    // XBE header.
    let header = XbeHeader {
        magic: *b"XBEH",
        base_address: BASE.into(),
        size_of_headers: 0x1000,
        size_of_image: SIZE_OF_IMAGE,
        num_sections: 2,
        section_headers_address: (BASE + TEXT_HEADER as u32).into(),
        entry: TEXT ^ DEBUG_ENTRY_KEY,
        kernel_image_thunk_addr: RDATA ^ DEBUG_THUNK_KEY,
        library_versions_addr: (BASE + LIBRARY_VERSIONS as u32).into(),
        ..Default::default()
    };
    write(&mut data, 0, &header);
    set_libraries(&mut data, &[("D3D8", 4054)]);

    // Section headers and their names.
    let names = BASE + SECTION_NAMES as u32;
    let text = XbeSectionHeader {
        flags: XbeSectionFlags::EXECUTABLE,
        virtual_address: TEXT.into(),
        virtual_size: 0x800,
        raw_address: TEXT_RAW,
        raw_size: 0x400,
        section_name_address: names.into(),
        ..Default::default()
    };
    let rdata = XbeSectionHeader {
        flags: XbeSectionFlags::empty(),
        virtual_address: RDATA.into(),
        virtual_size: 0x800,
        raw_address: RDATA_RAW,
        raw_size: 0x800,
        section_name_address: (names + 0x10).into(),
        ..Default::default()
    };
    write(&mut data, TEXT_HEADER, &text);
    write(&mut data, RDATA_HEADER, &rdata);
    data[SECTION_NAMES..][..6].copy_from_slice(b".text\0");
    data[SECTION_NAMES + 0x10..][..7].copy_from_slice(b".rdata\0");

    // The zero after the last thunk ends the table.
    for (index, &thunk) in thunks.iter().enumerate() {
        write_u32(&mut data, RDATA_RAW as usize + index * 4, thunk);
    }

    data
}

/// Loads `data` with the XBE loader.
pub(super) fn load(data: &[u8]) -> Executable {
    // SAFETY: `build_xbe` keeps every header offset in bounds. Tests only
    // change section fields, which the loader checks before reading.
    unsafe { load_executable(data) }
}

/// Replaces the library version table with `libraries`, given as
/// (name, build number) pairs.
pub(super) fn set_libraries(data: &mut [u8], libraries: &[(&str, u16)]) {
    let count_offset = core::mem::offset_of!(XbeHeader, library_versions);
    write_u32(data, count_offset, libraries.len() as u32);

    for (index, (name, build)) in libraries.iter().enumerate() {
        let mut entry = XbeLibraryVersion {
            build_version: *build,
            ..Default::default()
        };
        entry.name[..name.len()].copy_from_slice(name.as_bytes());
        let offset = LIBRARY_VERSIONS + index * XbeLibraryVersion::SIZE;
        write(data, offset, &entry);
    }
}

/// Writes `value` as little-endian at file offset `offset`.
pub(super) fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Writes `value` at file offset `offset`.
fn write<T: FileStruct>(data: &mut [u8], offset: usize, value: &T) {
    let bytes = &mut data[offset..offset + T::SIZE];
    // SAFETY: `bytes` is `T::SIZE` long, and `FileStruct` keeps writes in it.
    unsafe { LittleEndianWriter::new(bytes.as_mut_ptr()).write_at(value, 0) };
}
