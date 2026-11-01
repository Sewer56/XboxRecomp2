use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst), Some(imm)) = (
        register(&operands[0]).filter(|reg| is_mm0_to_7(*reg)),
        immediate(&operands[2]),
    ) else {
        return ctx.missing_form();
    };
    let src = &operands[1];
    let mmx = ctx.mmx();
    let source = match (register(src), memory(src)) {
        (Some(reg), _) if is_mm0_to_7(reg) => format!("{mmx}[{}]", mmx_index(reg)),
        (None, Some(_)) => format!("X86_LOAD_U64({})", ea(src)),
        _ => return ctx.missing_form(),
    };

    let dst = mmx_index(dst);
    let shuffle = imm.value & 0xFF;
    cpp!(
        ctx.out,
        "{mmx}[{dst}] = X86_PSHUFW<0x{shuffle:02X}>({source});"
    );
}
