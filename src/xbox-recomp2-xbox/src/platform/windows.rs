//! Special Xbox address ranges, used by [`super::ds`].

/// Start of the hardware registers (MMIO).
pub(super) const MMIO_BASE: u32 = 0xFE80_0000;
/// Size of the MMIO range in bytes.
pub(super) const MMIO_SIZE: u32 = 0x0080_0000;
/// Start of the GPU (NV2A) registers.
pub(super) const NV2A_DEVICE1_BASE: u32 = 0xFD00_0000;
/// Size of the NV2A range in bytes.
pub(super) const NV2A_DEVICE1_SIZE: u32 = 0x0070_0000;
/// Start of the kernel's view of physical RAM.
pub(super) const PHYSICAL_MAP_BASE: u32 = 0x8000_0000;
/// Size of the physical RAM view in bytes.
pub(super) const PHYSICAL_MAP_SIZE: u32 = 0x1000_0000;

/// Returns `true` for MMIO addresses, `0xFE800000` to `0xFEFFFFFF`.
#[inline]
pub(super) fn is_mmio_address(addr: u32) -> bool {
    (MMIO_BASE..MMIO_BASE + MMIO_SIZE).contains(&addr)
}

/// Returns `true` for NV2A addresses, `0xFD000000` to `0xFD6FFFFF`.
#[inline]
pub(super) fn is_nv2a_address(addr: u32) -> bool {
    (NV2A_DEVICE1_BASE..NV2A_DEVICE1_BASE + NV2A_DEVICE1_SIZE).contains(&addr)
}

/// Returns `true` for physical RAM view addresses, `0x80000000` to `0x8FFFFFFF`.
#[inline]
pub(super) fn is_physical_map_address(addr: u32) -> bool {
    (PHYSICAL_MAP_BASE..PHYSICAL_MAP_BASE + PHYSICAL_MAP_SIZE).contains(&addr)
}
