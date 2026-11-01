//! Header found in the XBE.
//!
//! Namely:
//! - Image Header: [`XbeHeader`]
//! - Library Version entries: [`XbeLibraryVersion`]
//! - Section Headers: [`XbeSectionHeader`]
//!
//! Decoded from the file layout.

bitflags! {
    /// Raw XBE section header flags, the `flags` field of
    /// [`XbeSectionHeader`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(transparent)]
    pub struct XbeSectionFlags: u32 {
        /// Section should be preloaded into memory.
        const PRELOAD = 0x1;

        /// Section is writable.
        const WRITABLE = 0x2;

        /// Section is executable.
        const EXECUTABLE = 0x4;

        /// Section was inserted by a packaging tool.
        const INSERTED_FILE = 0x8;

        /// The section header is preloaded before the section data.
        const PRELOAD_HEADER = 0x10;

        /// The section footer is preloaded after the section data.
        const PRELOAD_FOOTER = 0x20;

        /// All remaining bits; undocumented in the reference.
        const _ = !0;
    }
}

use bitflags::bitflags;
use endian_writer::{EndianReadableAt, EndianReader, EndianWritableAt, EndianWriter, HasSize};
use endian_writer_derive::EndianWritable;
use xbox_recomp2_executable::VirtualAddress;

/// Section names classified as data even when flagged executable.
///
/// Sewer (Note):
///
/// - Based on some quick research, it seems like many XBEs
///   mark data sections as executable.
/// - Disassembling them yields garbage. So we hardcoded this list.
///
/// Not ideal, but does the job for now.
///
/// Refs:
///
/// - caustik.com/cxbx/download/xbe.htm -> XBE spec; section flag bits.
/// - Cxbx-Reloaded `Xbe.h` -> `XBEIMAGE_SECTION_EXECUTABLE = 0x4`.
/// - xbdev.net/fileformats/xbe/code/J/output.txt -> retail dashboard
///   dump; `.data` = 0x07 and `DOLBY` = 0x26, both executable.
/// - sourceforge.net/p/dxbx/discussion/725856/thread/93fea0c6/ ->
///   retail Otogi dump; `.rdata` = 0x06, XDK libs = 0x07.
const DATA_SECTION_NAMES: [&str; 4] = [".rdata", ".data", "DOLBY", ".XTLID"];

/// The part of the XBE image header we need (368 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EndianWritable)]
#[repr(C)]
pub struct XbeHeader {
    /// Magic number, `XBEH`.
    pub magic: [u8; 4],

    /// RSA digital signature (bytes 4..260); not used.
    pub signature: [u8; 256],

    /// Base address of the executable image.
    pub base_address: VirtualAddress<u32>,

    /// Size of all headers.
    pub size_of_headers: u32,

    /// Size of the entire image.
    pub size_of_image: u32,

    /// Size of the image header.
    pub size_of_image_header: u32,

    /// Timestamp.
    pub timestamp: u32,

    /// Address of the certificate.
    pub certificate_address: VirtualAddress<u32>,

    /// Number of sections.
    pub num_sections: u32,

    /// Address of the section headers.
    pub section_headers_address: HeaderAddress,

    /// Init flags (bytes 0x124..0x128); not used.
    pub init_flags: u32,

    /// XOR-encoded entry point address.
    pub entry: u32,

    /// Thread-local storage address.
    pub tls_address: VirtualAddress<u32>,

    // Sewer note
    //
    // - TIL Xbox uses regular PE header as an intermediary.
    //
    //   Kinda curious what these fields are for.
    //   I'll find out someday, uwu
    //
    /// Original PE stack commit size.
    pub pe_stack_commit: u32,

    /// Original PE heap reserve size.
    pub pe_heap_reserve: u32,

    /// Original PE heap commit size.
    pub pe_heap_commit: u32,

    /// Original PE base address.
    pub pe_base_addr: VirtualAddress<u32>,

    /// Original PE image size.
    pub pe_sizeof_image: u32,

    /// Original PE checksum.
    pub pe_checksum: u32,

    /// Original PE timestamp.
    pub pe_timedate: u32,

    /// Debug pathname address.
    pub debug_pathname_addr: VirtualAddress<u32>,

    /// Debug filename address.
    pub debug_filename_addr: VirtualAddress<u32>,

    /// Debug unicode filename address.
    pub debug_unicode_filename_addr: VirtualAddress<u32>,

    /// XOR-encoded kernel thunk table address.
    pub kernel_image_thunk_addr: u32,

    /// Non-kernel import directory address.
    pub nonkernel_import_dir_addr: VirtualAddress<u32>,

    /// Number of library versions.
    pub library_versions: u32,

    /// Address of the library versions.
    pub library_versions_addr: HeaderAddress,

    /// Kernel library version address.
    pub kernel_library_version_addr: VirtualAddress<u32>,

    /// XAPI library version address.
    pub xapi_library_version_addr: VirtualAddress<u32>,
}

/// One XBE library-version entry (16 bytes).
///
/// Refs:
///
/// - Cxbx-Reloaded `Xbe.h` -> `XBEIMAGE_LIBRARY_VERSION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EndianWritable)]
#[repr(C)]
pub struct XbeLibraryVersion {
    /// NUL-padded library name, for example `D3D8`.
    pub name: [u8; 8],

    /// Library major version.
    pub major_version: u16,

    /// Library minor version.
    pub minor_version: u16,

    /// Library build version.
    pub build_version: u16,

    /// Library flags
    pub flags: u16,
}

/// One XBE section header (56 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EndianWritable)]
#[repr(C)]
pub struct XbeSectionHeader {
    /// Section flags; see [`XbeSectionFlags`].
    pub flags: XbeSectionFlags,

    /// Virtual address of the section.
    pub virtual_address: VirtualAddress<u32>,

    /// Virtual size of the section.
    pub virtual_size: u32,

    /// File offset of the section's raw data.
    pub raw_address: u32,

    /// File size of the section's raw data.
    pub raw_size: u32,

    /// Virtual address of the section name string.
    pub section_name_address: HeaderAddress,

    /// Section name reference count; not used.
    pub section_name_reference_count: u32,

    /// Head shared-page count; not used.
    pub head_shared_page_count: u32,

    /// Tail shared-page count; not used.
    pub tail_shared_page_count: u32,

    /// Section content digest; not used.
    pub section_digest: [u8; 20],
}

/// An address that points into the XBE headers.
///
/// The Xbox copies the headers straight from the start of the file to
/// [`XbeHeader::base_address`]. Subtract the base address to get the file
/// offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EndianWritable)]
#[repr(transparent)]
pub struct HeaderAddress {
    /// The address in the guest's virtual memory space.
    address: VirtualAddress<u32>,
}

impl XbeLibraryVersion {
    /// The library name.
    pub(super) fn name(&self) -> &[u8] {
        let end = self
            .name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(self.name.len());
        &self.name[..end]
    }
}

impl HeaderAddress {
    /// The address's file offset.
    ///
    /// # Arguments
    ///
    /// * `header` - the XBE header that provides the base address
    ///
    /// # Returns
    ///
    /// `self - header.base_address`, wrapping.
    pub(super) fn to_file_offset(self, header: &XbeHeader) -> u32 {
        self.address.wrapping_sub(header.base_address)
    }

    /// `true` when zero, the headers' "absent" marker.
    pub(super) fn is_zero(self) -> bool {
        self.address.is_zero()
    }
}

/// A zeroed header.
impl Default for XbeHeader {
    fn default() -> Self {
        Self {
            magic: [0; 4],
            signature: [0; 256],
            base_address: VirtualAddress::default(),
            size_of_headers: 0,
            size_of_image: 0,
            size_of_image_header: 0,
            timestamp: 0,
            certificate_address: VirtualAddress::default(),
            num_sections: 0,
            section_headers_address: HeaderAddress::default(),
            init_flags: 0,
            entry: 0,
            tls_address: VirtualAddress::default(),
            pe_stack_commit: 0,
            pe_heap_reserve: 0,
            pe_heap_commit: 0,
            pe_base_addr: VirtualAddress::default(),
            pe_sizeof_image: 0,
            pe_checksum: 0,
            pe_timedate: 0,
            debug_pathname_addr: VirtualAddress::default(),
            debug_filename_addr: VirtualAddress::default(),
            debug_unicode_filename_addr: VirtualAddress::default(),
            kernel_image_thunk_addr: 0,
            nonkernel_import_dir_addr: VirtualAddress::default(),
            library_versions: 0,
            library_versions_addr: HeaderAddress::default(),
            kernel_library_version_addr: VirtualAddress::default(),
            xapi_library_version_addr: VirtualAddress::default(),
        }
    }
}

impl From<u32> for HeaderAddress {
    fn from(address: u32) -> Self {
        Self {
            address: address.into(),
        }
    }
}

// The `EndianWritable` derive cannot apply to a `bitflags!` type
// (it generates a tuple struct), so we must derive it ourselves.
impl HasSize for XbeSectionFlags {
    const SIZE: usize = <u32 as HasSize>::SIZE;
}

impl EndianWritableAt for XbeSectionFlags {
    unsafe fn write_at<W: EndianWriter>(&self, writer: &mut W, offset: isize) {
        unsafe { self.bits().write_at(writer, offset) };
    }
}

impl EndianReadableAt for XbeSectionFlags {
    unsafe fn read_at<R: EndianReader>(reader: &mut R, offset: isize) -> Self {
        unsafe { Self::from_bits_retain(reader.read_u32_at(offset)) }
    }
}

/// Returns `true` if `name` is in [`DATA_SECTION_NAMES`].
pub(super) fn is_data_section(name: &str) -> bool {
    DATA_SECTION_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    //! Tests XBE struct layouts, header addresses and section flags.

    use super::*;
    use core::mem::offset_of;
    use endian_writer::{HasSize, LittleEndianReader};
    use rstest::rstest;

    /// Ensure our struct matches raw Xbe header since we parse 1:1
    #[rstest]
    #[case::section_address(offset_of!(XbeSectionHeader, virtual_address), 0x4)]
    #[case::section_size(offset_of!(XbeSectionHeader, virtual_size), 0x8)]
    #[case::library_build(offset_of!(XbeLibraryVersion, build_version), 0xC)]
    #[case::section_raw_address(offset_of!(XbeSectionHeader, raw_address), 0xC)]
    #[case::section_raw_size(offset_of!(XbeSectionHeader, raw_size), 0x10)]
    #[case::section_name(offset_of!(XbeSectionHeader, section_name_address), 0x14)]
    #[case::section_digest(offset_of!(XbeSectionHeader, section_digest), 0x24)]
    #[case::header_base_address(offset_of!(XbeHeader, base_address), 0x104)]
    #[case::header_size_of_image(offset_of!(XbeHeader, size_of_image), 0x10C)]
    #[case::header_certificate(offset_of!(XbeHeader, certificate_address), 0x118)]
    #[case::header_section_count(offset_of!(XbeHeader, num_sections), 0x11C)]
    #[case::header_sections(offset_of!(XbeHeader, section_headers_address), 0x120)]
    #[case::header_entry(offset_of!(XbeHeader, entry), 0x128)]
    #[case::header_kernel_thunks(offset_of!(XbeHeader, kernel_image_thunk_addr), 0x158)]
    #[case::header_library_count(offset_of!(XbeHeader, library_versions), 0x160)]
    #[case::header_library_table(offset_of!(XbeHeader, library_versions_addr), 0x164)]
    fn xbe_struct_offsets_are_correct(#[case] offset: usize, #[case] expected: usize) {
        assert_eq!(offset, expected);
    }

    /// Our struct sizes match, not only offsets.
    #[rstest]
    #[case::library(size_of::<XbeLibraryVersion>(), XbeLibraryVersion::SIZE, 0x10)]
    #[case::section(size_of::<XbeSectionHeader>(), XbeSectionHeader::SIZE, 0x38)]
    #[case::header(size_of::<XbeHeader>(), XbeHeader::SIZE, 0x170)]
    fn struct_sizes_are_correct(
        #[case] in_memory: usize,
        #[case] read_size: usize,
        #[case] expected: usize,
    ) {
        assert_eq!(in_memory, expected);
        assert_eq!(read_size, expected);
    }

    // For sanity, ensure that any flags we haven't set aren't lost.
    #[test]
    fn section_flags_should_keep_undocumented_bits_when_read() {
        let raw = XbeSectionFlags::EXECUTABLE.bits() | 0x8000_0000;
        let bytes = raw.to_le_bytes();

        let flags: XbeSectionFlags = unsafe {
            let mut reader = LittleEndianReader::new(bytes.as_ptr());
            reader.read_at(0)
        };

        assert!(flags.contains(XbeSectionFlags::EXECUTABLE));
        assert_eq!(flags.bits(), raw);
    }
}
