use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let checked = stack_template_argument(ctx);

    // Intel's manual leaves C0, C2 and C3 undefined after FLD, so we keep them.
    if memory(&operands[0]).is_some() {
        cpp!(ctx.out, "{fpu}.sw &= ~X87FPU::C1;");
        if operands[0].size == 32 {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "{fpu}.push{checked}(X86_LOAD_F32({addr}));");
        } else if operands[0].size == 64 {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "{fpu}.push{checked}(X86_LOAD_F64({addr}));");
        } else if operands[0].size == 80 {
            let addr = ea(&operands[0]);
            cpp!(ctx.out, "{fpu}.push{checked}(X86_LOAD_F80({addr}));");
        } else {
            ctx.missing_form();
        }
    } else if let Some(reg) = register(&operands[0]) {
        let idx = st_index(reg);
        cpp!(
            ctx.out,
            "
            {fpu}.sw &= ~X87FPU::C1;
            {fpu}.push{checked}({fpu}.st{idx}());
        "
        );
    } else {
        ctx.missing_form();
    }
}
