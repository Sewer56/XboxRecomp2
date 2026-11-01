//! Guest virtual addresses.
//!
//! Basically your in-memory RAM address, rather than raw executable
//! file offset.

use core::fmt::{self, Debug};
use core::hash::Hash;
use core::num::TryFromIntError;
use endian_writer_derive::EndianWritable;

/// A 32-bit guest virtual address.
pub type GuestAddress32 = VirtualAddress<u32>;

/// A 64-bit guest virtual address.
pub type GuestAddress64 = VirtualAddress<u64>;

/// An address in the guest's memory (RAM).
///
/// `T` is the guest's pointer size, either [`u32`] or [`u64`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, EndianWritable)]
#[repr(transparent)]
pub struct VirtualAddress<T: AddressWidth> {
    address: T,
}

/// An integer type that can hold a [`VirtualAddress`].
pub trait AddressWidth: Copy + Eq + Ord + Debug + Hash {
    /// Returns zero. a.k.a. 'no address'
    fn zero() -> Self;

    /// Converts a [`u32`] to this type.
    fn from_u32(value: u32) -> Self;

    /// Adds `other`, wrapping on overflow.
    fn wrapping_add(self, other: Self) -> Self;

    /// Subtracts `other`, wrapping on underflow.
    fn wrapping_sub(self, other: Self) -> Self;

    /// Widens the value to a `u64`.
    fn as_u64(self) -> u64;
}

impl<T: AddressWidth> VirtualAddress<T> {
    /// Returns the address as plain integer.
    ///
    /// # Returns
    ///
    /// The raw address, e.g. `0x8200_0000`.
    pub fn get(self) -> T {
        self.address
    }

    /// Returns `true` if the address is zero.
    ///
    /// Executable headers often use zero to mean "no address".
    pub fn is_zero(self) -> bool {
        self.address == T::zero()
    }

    /// How many bytes this address is past `base`.
    ///
    /// # Arguments
    ///
    /// * `base` - Address to measure from (e.g. a section's start).
    pub fn wrapping_sub(self, base: Self) -> T {
        // TODO: This API is weird, this returns T but sub returns 'Self'.
        //       Due to nature of the callers.
        self.address.wrapping_sub(base.address)
    }

    /// Adds `bytes` to this address, returning new address.
    ///
    /// # Arguments
    ///
    /// * `bytes` - How far to move forward, usually a field or section size.
    pub fn wrapping_add(self, bytes: u32) -> Self {
        Self {
            address: self.address.wrapping_add(T::from_u32(bytes)),
        }
    }
}

impl<T: AddressWidth + fmt::UpperHex> fmt::UpperHex for VirtualAddress<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.address, f)
    }
}

impl<T: AddressWidth + fmt::LowerHex> fmt::LowerHex for VirtualAddress<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.address, f)
    }
}

impl<T: AddressWidth> Default for VirtualAddress<T> {
    fn default() -> Self {
        Self { address: T::zero() }
    }
}

impl From<u32> for VirtualAddress<u32> {
    fn from(address: u32) -> Self {
        Self { address }
    }
}

impl From<u64> for VirtualAddress<u64> {
    fn from(address: u64) -> Self {
        Self { address }
    }
}

impl From<VirtualAddress<u32>> for VirtualAddress<u64> {
    fn from(address: VirtualAddress<u32>) -> Self {
        Self {
            address: address.address.as_u64(),
        }
    }
}

impl TryFrom<VirtualAddress<u64>> for VirtualAddress<u32> {
    type Error = TryFromIntError;

    fn try_from(address: VirtualAddress<u64>) -> Result<Self, Self::Error> {
        u32::try_from(address.address).map(Self::from)
    }
}

impl AddressWidth for u32 {
    fn zero() -> Self {
        0
    }

    fn from_u32(value: u32) -> Self {
        value
    }

    fn wrapping_add(self, other: Self) -> Self {
        self.wrapping_add(other)
    }

    fn wrapping_sub(self, other: Self) -> Self {
        self.wrapping_sub(other)
    }

    fn as_u64(self) -> u64 {
        u64::from(self)
    }
}

impl AddressWidth for u64 {
    fn zero() -> Self {
        0
    }

    fn from_u32(value: u32) -> Self {
        u64::from(value)
    }

    fn wrapping_add(self, other: Self) -> Self {
        self.wrapping_add(other)
    }

    fn wrapping_sub(self, other: Self) -> Self {
        self.wrapping_sub(other)
    }

    fn as_u64(self) -> u64 {
        self
    }
}

impl<T: AddressWidth> From<VirtualAddress<T>> for u64 {
    fn from(address: VirtualAddress<T>) -> Self {
        address.address.as_u64()
    }
}
