//! Handles `ds:[...]` accesses to special Xbox addresses.
//!
//! - Hardware (MMIO) and GPU (NV2A) registers use `g_mmio` and `g_nv2aPtr`.
//! - `mov` skips writes to physical RAM.

use super::windows::{
    MMIO_BASE, NV2A_DEVICE1_BASE, is_mmio_address, is_nv2a_address, is_physical_map_address,
};
use xbox_recomp2::platform::MemoryRoute;
use xbox_recomp2_executable::ImportMap;
use zydis::Mnemonic;

/// Routes one `ds:[address]` access.
pub(super) fn route(
    imports: &ImportMap,
    insn: Mnemonic,
    address: u32,
    size: u16,
    is_write: bool,
) -> MemoryRoute {
    match insn {
        Mnemonic::MOV => {
            if is_mmio_address(address) {
                hardware_statement(&host_array(MMIO_BASE, "g_mmio", address, size), is_write)
            } else if is_nv2a_address(address) {
                hardware_statement(
                    &host_array(NV2A_DEVICE1_BASE, "g_nv2aPtr", address, size),
                    is_write,
                )
            } else if is_write && is_physical_map_address(address) {
                MemoryRoute::Statement("\t//skip write in physical memory".to_string())
            } else if imports.is_data_import(address.into()) {
                MemoryRoute::Expr("XboxSignatureKey[{reg}]".to_string())
            } else {
                MemoryRoute::Plain
            }
        }
        Mnemonic::TEST if is_mmio_address(address) => {
            MemoryRoute::Expr(host_array(MMIO_BASE, "g_mmio", address, size))
        }
        Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR if imports.is_data_import(address.into()) => {
            MemoryRoute::Expr("XboxSignatureKey[{reg}]".to_string())
        }
        _ => MemoryRoute::Plain,
    }
}

/// Reads `array` into `{reg}`, or writes `{reg}` to it.
fn hardware_statement(array: &str, is_write: bool) -> MemoryRoute {
    if is_write {
        MemoryRoute::Statement(format!(
            "\t{array} = {{reg}};  //Write in DS Hardware memory"
        ))
    } else {
        MemoryRoute::Statement(format!("\t{{reg}} = {array};  //Read DS Hardware variable"))
    }
}

/// Accesses `size` bits at `name[addr - base]`.
/// e.g. `*(uint32_t*)&g_mmio[16]`.
fn host_array(base: u32, name: &str, addr: u32, size: u16) -> String {
    let offset = addr - base;

    match size {
        8 => format!("{name}[{offset}]"),
        16 => format!("*(uint16_t*)&{name}[{offset}]"),
        32 => format!("*(uint32_t*)&{name}[{offset}]"),
        _ => format!("/* error offset {name}*/;"),
    }
}
