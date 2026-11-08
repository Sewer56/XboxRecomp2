use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];
    let src = &operands[1];

    if let Some(dst_reg) = register(dst).filter(|reg| is_mm0_to_7(*reg) || is_xmm0_to_7(*reg)) {
        let value = match (register(src), memory(src)) {
            (Some(reg), _) if reg.class() == RegisterClass::GPR32 => get_x86_reg(reg),
            (None, Some(_)) => format!("X86_LOAD_U32({})", ctx.ea(src)),
            _ => return ctx.missing_form(),
        };
        if is_mm0_to_7(dst_reg) {
            let dst = ctx.mmx_reg(mmx_index(dst_reg));
            cpp!(ctx.out, "{dst} = static_cast<uint64_t>({value});");
        } else {
            let dst_idx = xmm_index(dst_reg);
            cpp!(ctx.out, "XMM_U32({dst_idx}, 0) = {value};");
            for lane in 1..4 {
                cpp!(ctx.out, "XMM_U32({dst_idx}, {lane}) = 0;");
            }
        }
        return;
    }

    let value = match register(src) {
        Some(reg) if is_mm0_to_7(reg) => {
            format!("static_cast<uint32_t>({})", ctx.mmx_reg(mmx_index(reg)))
        }
        Some(reg) if is_xmm0_to_7(reg) => format!("XMM_U32({}, 0)", xmm_index(reg)),
        _ => return ctx.missing_form(),
    };
    match (register(dst), memory(dst)) {
        (Some(reg), _) if reg.class() == RegisterClass::GPR32 => {
            let dst_str = get_x86_reg(reg);
            cpp!(ctx.out, "{dst_str} = {value};");
        }
        (None, Some(_)) => {
            let addr = ctx.ea(dst);
            cpp!(ctx.out, "X86_STORE_U32({addr}, {value});");
        }
        _ => ctx.missing_form(),
    }
}
