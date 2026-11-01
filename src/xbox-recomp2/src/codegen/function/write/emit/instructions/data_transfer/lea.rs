use super::super::prelude::*;
use crate::decode::MemoryInfo;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    let (Some(dst), Some(mem)) = (register(&operands[0]), memory(&operands[1])) else {
        return ctx.missing_form();
    };
    // LEA reg, [mem] -> reg = address calculation only
    let dst_str = get_x86_reg(dst);
    let address = if ctx.insn.instruction().address_width == 16 {
        address16(mem)
    } else {
        ea(&operands[1])
    };
    cpp!(ctx.out, "{dst_str} = {address};");
}

fn address16(mem: &MemoryInfo) -> String {
    let mut parts: Vec<String> = [mem.base, mem.index]
        .into_iter()
        .filter(|reg| *reg != Register::NONE)
        .map(get_x86_reg)
        .collect();
    if mem.disp.displacement != 0 || parts.is_empty() {
        parts.push(format!("0x{:X}", mem.disp.displacement as u16));
    }
    format!("(uint16_t)({})", parts.join(" + "))
}
