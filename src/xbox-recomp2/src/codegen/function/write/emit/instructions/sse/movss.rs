use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (dst, src) = (&operands[0], &operands[1]);
    match (xmm_register(dst), xmm_register(src)) {
        (Some(reg_idx), None) if memory(src).is_some() => {
            let addr = ctx.ea(src);
            cpp!(
                ctx.out,
                "ctx.xmm[{reg_idx}].reg = _mm_set_ss(X86_LOAD_F32({addr}));"
            );
        }
        (None, Some(reg_idx)) if memory(dst).is_some() => {
            let addr = ctx.ea(dst);
            cpp!(ctx.out, "X86_STORE_F32({addr}, XMM_F32({reg_idx}, 0));");
        }
        (Some(dst_idx), Some(src_idx)) => {
            cpp!(ctx.out, "XMM_F32({dst_idx}, 0) = XMM_F32({src_idx}, 0);");
        }
        _ => ctx.missing_form(),
    }
}
