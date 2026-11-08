//! A guest `idiv r/m32` after `cdq`.

use super::super::GuestOp;

/// A guest `idiv r/m32` after `cdq`, with EAX and EDX unchanged between them.
/// i.e. a sign-extended 32-bit number.
///
/// We can skip some calculations here, so this is an intrinsic.
#[derive(Clone, Debug)]
pub(crate) struct CdqIdiv {
    /// The original instruction, e.g. `idiv ebx`.
    pub guest: GuestOp,
}
