use super::super::prelude::*;
use super::helpers::{packed_prefix, xmm_register};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (dst, src) = (&operands[0], &operands[1]);
    match (xmm_register(dst), xmm_register(src)) {
        // Register to register move
        (Some(dst_reg), Some(src_reg)) => {
            cpp!(ctx.out, "ctx.xmm[{dst_reg}].reg = ctx.xmm[{src_reg}].reg;");
        }
        // Memory to register (aligned load)
        (Some(dst_reg), None) if memory(src).is_some() => {
            let addr = ctx.ea(src);
            let prefix = packed_prefix(ctx);
            cpp!(
                ctx.out,
                "ctx.xmm[{dst_reg}].reg = {prefix}LOAD_APS({addr});"
            );
        }
        // Register to memory (aligned store)
        (None, Some(src_reg)) if memory(dst).is_some() => {
            let addr = ctx.ea(dst);
            let prefix = packed_prefix(ctx);
            cpp!(
                ctx.out,
                "{prefix}STORE_APS({addr}, ctx.xmm[{src_reg}].reg);"
            );
        }
        _ => ctx.missing_form(),
    }
}
