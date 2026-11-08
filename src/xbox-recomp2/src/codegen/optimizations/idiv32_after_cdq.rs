//! Marks each `idiv r/m32` right after `cdq`, so it gets a 32-bit divide.
//!
//! See `idiv-after-cdq.md` for more info.

use crate::codegen::function::model::{CdqIdiv, FunctionModel, GuestOp, Hook, Op, StepFailure};
use crate::decode::Mnemonic;
use core::mem;
use xbox_recomp2_executable::GuestAddress32;

/// Turns each `idiv r/m32` right after `cdq` into [`Op::CdqIdiv`].
pub(super) fn run(model: &mut FunctionModel) {
    for block in &mut model.blocks {
        let mut after_cdq = false;
        for op in &mut block.ops {
            let make = after_cdq && is_unhooked_idiv32(op, &model.hooks);

            // Remember if this op is `cdq`, for the next op.
            after_cdq = is_cdq(op);

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
    fn run_should_mark_idiv_only_right_after_cdq() {
        // Arrange
        #[rustfmt::skip]
        let code = [
            0x99,                   // 1000  cdq
            0xF7, 0xFB,             // 1001  idiv ebx    ; right after cdq: marked
            0x99,                   // 1003  cdq
            0x90,                   // 1004  nop
            0xF7, 0xFB,             // 1005  idiv ebx    ; nop in between: left alone
            0xC3,                   // 1007  ret
        ];
        let mut model = model_of(&code);
        let expected = model.address.0 + 1;

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
