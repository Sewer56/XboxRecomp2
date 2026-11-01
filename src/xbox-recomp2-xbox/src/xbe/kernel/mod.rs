//! Reads the kernel thunk table (import table) of an XBE.
//!
//! The ordinal-to-name list lives in [`exports`].

use endian_writer::{EndianReader, LittleEndianReader};
use exports::KERNEL_EXPORTS;
use xbox_recomp2_executable::{Executable, Symbol, SymbolType, VirtualAddress};

mod exports;

/// Maximum number of thunk slots to read.
const MAX_THUNK_ENTRIES: u32 = 366;
/// Ordinal of `XboxSignatureKey`, which games read as data.
const SIGNATURE_ORDINAL: u16 = 0x145;
/// Ordinals of kernel variables.
#[rustfmt::skip]
const VARIABLE_ORDINALS: [u16; 34] = [
    0x0010, // ExEventObjectType
    0x0016, // ExMutantObjectType
    0x001E, // ExSemaphoreObjectType
    0x001F, // ExTimerObjectType
    0x0028, // HalDiskCachePartitionCount
    0x0029, // HalDiskModelNumber
    0x002A, // HalDiskSerialNumber
    0x0040, // IoCompletionObjectType
    0x0046, // IoDeviceObjectType
    0x0047, // IoFileObjectType
    0x0058, // KdDebuggerEnabled
    0x0059, // KdDebuggerNotPresent
    0x0066, // MmGlobalData
    0x0078, // KeInterruptTime
    0x009A, // KeSystemTime
    0x009C, // KeTickCount
    0x009D, // KeTimeIncrement
    0x00A2, // KiBugCheckData
    0x00A4, // LaunchDataPage
    0x00F0, // ObDirectoryObjectType
    0x00F5, // ObpObjectHandleTable
    0x00F9, // ObSymbolicLinkObjectType
    0x0103, // PsThreadObjectType
    0x0141, // XboxEEPROMKey
    0x0142, // XboxHardwareInfo
    0x0143, // XboxHDKey
    0x0144, // XboxKrnlVersion
    0x0145, // XboxSignatureKey
    0x0146, // XeImageFileName
    0x0161, // XboxLANKey
    0x0162, // XboxAlternateSignatureKeys
    0x0163, // XePublicKeyData
    0x0164, // HalBootSMCVideoMode
    0x0165, // IdexChannelObject
];

/// Loads kernel imports into `executable` from the kernel thunk table
/// (import table).
///
/// The thunk table is a list of 4-byte slots, usually in `.rdata`.
/// Games call kernel functions using mechanism shown below.
///
/// Example Shadow the Hedgehog (NTSC-U):
///
/// 1. Code calls through a slot:
///
///     ```text
///     0x002AC43E:  FF 15 E4 9A 42 00   ; call dword ptr [0x00429AE4]
///     ```
///
/// 2. The slot holds an ordinal, not an address:
///
///     ```text
///     0x00429AE4:  0x80000115          ; flag bits + ordinal 0x115
///     ```
///
/// 3. Ordinal `0x115` is `RtlEnterCriticalSection`.
///
/// This function does steps 2 and 3 for every slot.
///
/// We resolve each call into a `__imp__<name>` symbol.
/// Allowing us to call the functions directly.
pub(super) fn load_kernel_functions(
    executable: &mut Executable,
    kernel_func_table: VirtualAddress<u32>,
) {
    if kernel_func_table.is_zero() {
        return;
    }

    // Find the section holding the table.
    let Some(section) = executable.section_containing(kernel_func_table) else {
        return;
    };
    let offset = kernel_func_table.get() as u64 - section.base.get();
    let Some(table) = section.file_bytes(&executable.data).get(offset as usize..) else {
        return;
    };

    // Read one slot at a time.
    let mut reader = unsafe { LittleEndianReader::new(table.as_ptr()) };
    let mut thunks: Vec<Symbol> = Vec::with_capacity(MAX_THUNK_ENTRIES as usize);
    for index in 0..(table.len() / 4).min(MAX_THUNK_ENTRIES as usize) {
        // Step 2: read the slot's ordinal.
        let slot = unsafe { reader.read_u32() };

        if slot & 0xFF == 0 {
            break;
        }

        // Step 3: look up the ordinal's name.
        let ordinal = (slot & 0xFFFF) as u16;
        let slot_addr = kernel_func_table.wrapping_add(index as u32 * 4);

        if let Some((_, name)) = KERNEL_EXPORTS.iter().find(|(known, _)| *known == ordinal)
            && !VARIABLE_ORDINALS.contains(&ordinal)
        {
            executable
                .imports
                .add_thunk(slot_addr.get().into(), slot.into());
            thunks.push(Symbol {
                name: format!("__imp__{name}").into(),
                address: slot_addr.into(),
                size: 1,
                kind: SymbolType::Function,
            });
        }

        if ordinal == SIGNATURE_ORDINAL {
            executable.imports.add_data_import(slot_addr.get().into());
        }
    }

    executable.symbols.insert_batch(thunks);
}

#[cfg(test)]
mod tests {
    use crate::xbe::header::XbeSectionHeader;
    use crate::xbe::test_helpers::*;
    use core::mem::offset_of;
    use rstest::rstest;
    use xbox_recomp2_executable::{ImportToken, SymbolType};

    /// `DbgPrint`, a regular kernel function.
    const DBG_PRINT: u32 = 0x8;
    /// `ExEventObjectType`, a kernel variable. It shouldn't get a thunk.
    const EVENT_OBJECT_TYPE: u32 = 0x10;
    /// `XboxSignatureKey`, a kernel variable that games read as data.
    const SIGNATURE_KEY: u32 = 0x145;

    #[test]
    fn kernel_imports_should_load() {
        let data = build_xbe(&[DBG_PRINT, EVENT_OBJECT_TYPE, SIGNATURE_KEY]);

        let executable = load(&data);

        let imports = &executable.imports;
        assert_eq!(
            imports.resolve_thunk(RDATA.into()),
            Some(ImportToken(DBG_PRINT))
        );
        assert_eq!(imports.resolve_thunk((RDATA + 4).into()), None);
        assert!(imports.is_data_import((RDATA + 8).into()));

        let symbols: Vec<_> = executable.symbols.iter().collect();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name.as_ref(), "__imp__DbgPrint");
        assert_eq!(symbols[0].address.get(), u64::from(RDATA));
        assert_eq!(symbols[0].kind, SymbolType::Function);
    }

    /// Two `DbgPrint` slots, but `raw_size` cuts the section short.
    ///
    /// With 7 bytes, the second slot is missing its last byte, so we skip it.
    /// With 0 bytes, there's nothing to read at all.
    #[rstest]
    #[case::partial_second_slot(7, 1)]
    #[case::nothing_stored(0, 0)]
    fn kernel_imports_should_stop_at_stored_bytes(#[case] raw_size: u32, #[case] expected: usize) {
        let mut data = build_xbe(&[DBG_PRINT, DBG_PRINT]);
        let field = offset_of!(XbeSectionHeader, raw_size);
        write_u32(&mut data, RDATA_HEADER + field, raw_size);

        let executable = load(&data);

        assert_eq!(executable.symbols.iter().count(), expected);
        assert_eq!(executable.imports.resolve_thunk((RDATA + 4).into()), None);
    }
}
