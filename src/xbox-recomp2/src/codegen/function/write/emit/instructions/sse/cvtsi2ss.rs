use super::super::prelude::*;
use super::helpers::xmm_register;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    // CVTSI2SS xmm, reg32/m32
    let Some(dst_reg) = xmm_register(&operands[0]) else {
        return ctx.missing_form();
    };
    let dst_reg = format!("ctx.xmm[{dst_reg}].reg");
    let src = &operands[1];

    match (register(src), memory(src)) {
        (Some(src_reg), _) if src_reg.class() == RegisterClass::GPR32 => {
            let src_reg = get_x86_reg(src_reg);
            cpp!(ctx.out, "{dst_reg} = _mm_cvtsi32_ss({dst_reg}, {src_reg});");
        }
        (None, Some(_)) => {
            let addr = ea(src);
            cpp!(
                ctx.out,
                "{dst_reg} = _mm_cvtsi32_ss({dst_reg}, (int32_t)X86_LOAD_U32({addr}));"
            );
        }
        _ => ctx.missing_form(),
    }
}
