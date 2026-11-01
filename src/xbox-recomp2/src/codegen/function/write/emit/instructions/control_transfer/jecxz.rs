use super::super::prelude::*;
use super::helpers::print_conditional_branch;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    if usize::from(ctx.insn.instruction().operand_count) >= 1 {
        let cond = if ctx.insn.mnemonic() == Mnemonic::JCXZ {
            "ctx.ecx.u16 == 0"
        } else {
            "ctx.ecx.u32 == 0"
        };
        print_conditional_branch(ctx, false, cond);
    }
}
