//! Marks each `idiv r/m32` after `cdq`, so it gets a 32-bit divide.
//!
//! See `idiv-after-cdq.md` for more info.

use super::helpers::instruction::register_access::RegisterAccess;
use crate::codegen::function::model::{CdqIdiv, FunctionModel, GuestOp, Hook, Op, StepFailure};
use crate::decode::{Mnemonic, Register};
use core::mem;
use xbox_recomp2_executable::GuestAddress32;

/// Turns each `idiv r/m32` after `cdq` into [`Op::CdqIdiv`],
/// if nothing between them changes EAX or EDX.
pub(super) fn run(model: &mut FunctionModel) {
    for block in &mut model.blocks {
        let mut after_cdq = false;
        for op in &mut block.ops {
            let make = after_cdq && is_unhooked_idiv32(op, &model.hooks);

            // Remember the `cdq` until something may change EAX or EDX.
            after_cdq = is_cdq(op) || (after_cdq && keeps_eax_edx(op, &model.hooks));

            if make {
                make_intrinsic(op);
            }
        }
    }
}

/// Turns the guest `idiv` in `op` into [`Op::CdqIdiv`].
fn make_intrinsic(op: &mut Op) {
    let placeholder = Op::Failed {
        address: GuestAddress32::default(),
        failure: StepFailure::Unmapped,
    };

    *op = match mem::replace(op, placeholder) {
        Op::Guest(guest) => Op::CdqIdiv(CdqIdiv { guest }),
        other => other,
    };
}

/// Whether `op` is a guest `cdq`.
fn is_cdq(op: &Op) -> bool {
    match op {
        Op::Guest(guest) => guest.insn.mnemonic() == Mnemonic::CDQ,
        _ => false,
    }
}

/// Whether `op` is an unhooked guest op that leaves EAX and EDX alone.
fn keeps_eax_edx(op: &Op, hooks: &[Hook]) -> bool {
    let guest = match op {
        Op::Guest(guest) => guest,
        _ => return false,
    };
    if is_hooked(guest, hooks) {
        return false;
    }

    let access = RegisterAccess::of(&guest.insn);
    !access.writes(Register::EAX) && !access.writes(Register::EDX)
}

/// Whether `op` is a guest `idiv r/m32` with no mid-assembly hook.
fn is_unhooked_idiv32(op: &Op, hooks: &[Hook]) -> bool {
    match op {
        Op::Guest(guest) => is_idiv32(guest) && !is_hooked(guest, hooks),
        _ => false,
    }
}

/// Whether `guest` is `idiv r/m32`.
fn is_idiv32(guest: &GuestOp) -> bool {
    if guest.insn.mnemonic() != Mnemonic::IDIV {
        return false;
    }

    let operands = guest.insn.operands();
    operands.first().is_some_and(|operand| operand.size == 32)
}

/// Whether a mid-assembly hook runs before `guest`.
fn is_hooked(guest: &GuestOp, hooks: &[Hook]) -> bool {
    hooks.iter().any(|hook| hook.address == guest.address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;

    #[test]
    fn run_should_mark_idiv_only_when_eax_and_edx_are_kept() {
        // Arrange
        #[rustfmt::skip]
        let code = [
            0x99,                   // 1000  cdq
            0x6A, 0x0C,             // 1001  push 0x0C
            0x5E,                   // 1003  pop  esi
            0xF7, 0xFE,             // 1004  idiv esi    ; EAX, EDX kept: marked
            0x99,                   // 1006  cdq
            0x31, 0xD2,             // 1007  xor  edx, edx
            0xF7, 0xFE,             // 1009  idiv esi    ; EDX changed: left alone
            0xC3,                   // 100B  ret
        ];
        let mut model = model_of(&code);
        let expected = model.address.0 + 4;

        // Act
        run(&mut model);

        // Assert
        let marked: Vec<u32> = model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .filter_map(|op| match op {
                Op::CdqIdiv(idiv) => Some(idiv.guest.address.get()),
                _ => None,
            })
            .collect();
        assert_eq!(marked, [expected]);
    }
}
