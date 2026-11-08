use super::super::prelude::*;
use super::helpers::stack_template_argument;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let fpu = ctx.fpu();
    let operands = ctx.insn.operands();
    let checked = stack_template_argument(ctx);

    if let Some(reg) = register(&operands[0]) {
        let dst_idx = st_index(reg);

        if dst_idx == 0 {
            cpp!(ctx.out, "{fpu}.pop{checked}();");
        } else {
            cpp!(
                ctx.out,
                "
                {fpu}.st{dst_idx}() = {fpu}.st0();
                {fpu}.pop{checked}();
            "
            );
        }
    } else if memory(&operands[0]).is_some() && matches!(operands[0].size, 32 | 64 | 80) {
        if operands[0].size == 32 {
            let addr = ctx.ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_F32({addr}, {fpu}.st0());");
        } else if operands[0].size == 64 {
            let addr = ctx.ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_F64({addr}, {fpu}.st0());");
        } else if operands[0].size == 80 {
            let addr = ctx.ea(&operands[0]);
            cpp!(ctx.out, "X86_STORE_F80({addr}, {fpu}.st0());");
        }

        cpp!(ctx.out, "{fpu}.pop{checked}();");
    } else {
        ctx.missing_form();
    }
}
