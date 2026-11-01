//! Just one struct for now, a function! whoa!

use xbox_recomp2_executable::GuestAddress64;

/// A discovered function: base address and size in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Function {
    /// Guest base address of the function.
    pub base: GuestAddress64,

    /// Size of the function in bytes.
    pub size: u64,
}
