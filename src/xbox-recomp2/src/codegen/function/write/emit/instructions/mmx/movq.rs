use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let dst = &operands[0];
    let src = &operands[1];
    let class = |operand: &DecodedOperand| register(operand).map(|reg| reg.class());
    let mmx = ctx.mmx();

    match (
        class(dst),
        class(src),
        memory(dst).is_some(),
        memory(src).is_some(),
    ) {
        (Some(RegisterClass::MMX), Some(RegisterClass::MMX), _, _) => {
            let dst_idx = mmx_index(register(dst).unwrap());
            let src_idx = mmx_index(register(src).unwrap());
            cpp!(ctx.out, "{mmx}[{dst_idx}] = {mmx}[{src_idx}];");
        }
        // MMX to memory
        (None, Some(RegisterClass::MMX), true, _) => {
            let addr = ctx.ea(dst);
            let src_idx = mmx_index(register(src).unwrap());
            cpp!(ctx.out, "X86_STORE_U64({addr}, {mmx}[{src_idx}]);");
        }
        // Memory to MMX
        (Some(RegisterClass::MMX), None, _, true) => {
            let dst_idx = mmx_index(register(dst).unwrap());
            let addr = ctx.ea(src);
            cpp!(ctx.out, "{mmx}[{dst_idx}] = X86_LOAD_U64({addr});");
        }
        // XMM to memory (lower 64 bits only)
        (None, Some(RegisterClass::XMM), true, _) => {
            let addr = ctx.ea(dst);
            let src_idx = xmm_index(register(src).unwrap());
            cpp!(ctx.out, "X86_STORE_U64({addr}, ctx.xmm[{src_idx}].u64[0]);");
        }
        // Memory to XMM (lower 64 bits; zero upper)
        (Some(RegisterClass::XMM), None, _, true) => {
            let index = xmm_index(register(dst).unwrap());
            let addr = ctx.ea(src);
            cpp!(
                ctx.out,
                "
                ctx.xmm[{index}].u64[0] = X86_LOAD_U64({addr});
                ctx.xmm[{index}].u64[1] = 0;
            "
            );
        }
        // XMM to XMM (lower 64 bits only)
        (Some(RegisterClass::XMM), Some(RegisterClass::XMM), _, _) => {
            let dst_idx = xmm_index(register(dst).unwrap());
            let src_idx = xmm_index(register(src).unwrap());
            cpp!(
                ctx.out,
                "
                ctx.xmm[{dst_idx}].u64[0] = ctx.xmm[{src_idx}].u64[0];
                ctx.xmm[{dst_idx}].u64[1] = 0;
            "
            );
        }
        _ => ctx.missing_form(),
    }
}
