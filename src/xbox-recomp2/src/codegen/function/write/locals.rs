//! Finds the temporary variables, like `val_uint32_0`, a function's C++ needs.

use crate::codegen::function::model::{Block, Local, Op};
use crate::decode::{Insn, InstructionAttributes, Mnemonic};

/// Lists the variables `blocks` need, once each, in first-use order.
pub(super) fn get_variable_declarations(blocks: &[Block]) -> Vec<Local> {
    let mut locals = Vec::new();
    for op in blocks.iter().flat_map(|block| &block.ops) {
        match op {
            Op::Guest(guest) => add_locals_for_insn(&guest.insn, &mut locals),
            Op::Call(call) => add_locals_for_insn(&call.guest.insn, &mut locals),
            Op::CdqIdiv(idiv) => add_locals_for_insn(&idiv.guest.insn, &mut locals),
            Op::MergedCmpJmp(merged) => {
                add_locals_for_insn(&merged.compare.insn, &mut locals);
                for instruction in &merged.instructions {
                    add_locals_for_insn(&instruction.guest.insn, &mut locals);
                }
            }
            _ => {}
        }
    }
    locals
}

/// Adds the variables `insn`'s C++ uses that aren't in `locals` yet.
fn add_locals_for_insn(insn: &Insn, locals: &mut Vec<Local>) {
    let has_rep = insn
        .instruction()
        .attributes
        .contains(InstructionAttributes::HAS_REP);

    let needed: &[Local] = match insn.mnemonic() {
        // Floating-point math and compares that keep one value aside.
        //
        // e.g. `val_double_0 = ctx.fpu.st0() + ctx.fpu.st1();`
        Mnemonic::FADD
        | Mnemonic::FSUB
        | Mnemonic::FMUL
        | Mnemonic::FDIV
        | Mnemonic::FIADD
        | Mnemonic::FSIN
        | Mnemonic::FCOS
        | Mnemonic::F2XM1
        | Mnemonic::FSCALE
        | Mnemonic::FCOMIP
        | Mnemonic::FUCOMIP => &[Local::Double(0)],

        // `_2` holds the input; `_0` and `_1` get the sine and cosine.
        //
        // e.g. `val_double_0 = std::sin(val_double_2);`
        Mnemonic::FSINCOS => &[Local::Double(0), Local::Double(1), Local::Double(2)],

        // Indirect calls keep the target address. Direct calls don't use it.
        //
        // e.g. `val_uint32_0 = ctx.eax.u32;`
        Mnemonic::CALL => &[Local::Uint32(0)],

        // `rep movsw` counts down a copy of `ecx`. A single `movsw` needs none.
        //
        // e.g. `for (; val_uint32_0 > 0; val_uint32_0--) { ... }`
        Mnemonic::MOVSW if has_rep => &[Local::Uint32(0)],

        // The two values `cmpsd` loads from `esi` and `edi` to compare.
        //
        // e.g. `val_uint32_0 = X86_LOAD_U32(ctx.esi.u32);`
        Mnemonic::CMPSD => &[Local::Uint32(0), Local::Uint32(1)],

        // The two 64-bit inputs whose halves get combined.
        //
        // e.g. `MMX_REG(0) = val_uint64_0 | (val_uint64_1 << 32);`
        Mnemonic::PUNPCKLDQ | Mnemonic::PUNPCKHDQ => &[Local::Uint64(0), Local::Uint64(1)],

        _ => &[],
    };

    for &local in needed {
        if !locals.contains(&local) {
            locals.push(local);
        }
    }
}
