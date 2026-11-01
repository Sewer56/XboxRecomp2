use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if usize::from(ctx.insn.instruction().operand_count) != 2 {
        return ctx.missing_form();
    }
    let dst = &operands[0];
    let src = &operands[1];

    // The other half stays unchanged.
    match (register(dst), memory(dst), register(src), memory(src)) {
        // movlps xmm, [mem]
        (Some(reg), None, None, Some(_)) if is_xmm0_to_7(reg) => {
            let reg = xmm_index(reg);
            let addr = ea(src);
            // Legacy moves allow unaligned memory on every host.
            cpp!(
                ctx.out,
                "{{ typedef uint64_t UnalignedU64 __attribute__((aligned(1))); ctx.xmm[{reg}].u64[0] = *reinterpret_cast<volatile UnalignedU64*>(base + ({addr})); }}"
            );
        }
        // movlps [mem], xmm
        (None, Some(_), Some(reg), None) if is_xmm0_to_7(reg) => {
            let reg = xmm_index(reg);
            let addr = ea(dst);
            cpp!(
                ctx.out,
                "{{ typedef uint64_t UnalignedU64 __attribute__((aligned(1))); *reinterpret_cast<volatile UnalignedU64*>(base + ({addr})) = ctx.xmm[{reg}].u64[0]; }}"
            );
        }
        _ => ctx.missing_form(),
    }
}
