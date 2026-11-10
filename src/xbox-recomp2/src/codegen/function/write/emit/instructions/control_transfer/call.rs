use super::super::prelude::*;
use super::helpers::print_function_call_addr;
use crate::codegen::function::model::Call;
use crate::codegen::optimizations::helpers::stack_frame::STACK_WORD_BYTES;

pub(in crate::codegen::function::write) fn emit(ctx: &mut Emitter<'_>, call: &Call) {
    // Calls skip `lookup`, so check the address forms here too.
    let operands = ctx.insn.operands();
    if operands[0].size == 48
        || matches!(operands[0].kind, DecodedOperandKind::Ptr(_))
        || !super::super::addressing::is_supported(ctx.insn)
    {
        return ctx.missing_form();
    }

    // Direct call (relative offset)
    if immediate(&operands[0]).is_some() {
        // The return address; and any other stack space for arguments.
        // Usually 4 if not optimised, but may be more due to optimisation
        // passes.
        let words = call.stack_bytes / STACK_WORD_BYTES;
        if words != 0 {
            cpp!(
                ctx.out,
                "ctx.SubEsp({words}); //Adjust stack pointer for call instruction"
            );
        }
        let target = ctx
            .base
            .wrapping_add(ctx.insn.length() as u32)
            .wrapping_add(immediate(&operands[0]).unwrap().value as i64 as u32);

        ctx.x87_local_to_ctx();
        if print_function_call_addr(ctx, target, call.calls_fastcall) {
            ctx.x87_ctx_to_local();
        } else {
            if words != 0 {
                cpp!(
                    ctx.out,
                    "ctx.AddEsp({words}); //function wasn't found, reAdjust stack pointer TODO FIX!!!!!"
                );
            }
            if ctx.config.debug {
                let base = ctx.base.get();
                cpp!(
                    ctx.out,
                    "printf(\"MISSING FUNCTION CALL: {target:X} at {base:X}\\n\");"
                );
            }
        }
    } else if memory(&operands[0]).is_some() {
        let target = GuestAddress32::from(memory(&operands[0]).unwrap().disp.displacement as u32);
        if ctx.imports.resolve_thunk(target.get().into()).is_some() {
            cpp!(
                ctx.out,
                "ctx.SubEsp(1); //Adjust stack pointer for call instruction"
            );
            ctx.x87_local_to_ctx();
            if print_function_call_addr(ctx, target, false) {
                ctx.x87_ctx_to_local();
            } else {
                cpp!(
                    ctx.out,
                    "ctx.AddEsp(1); //function wasn't found, reAdjust stack pointer TODO FIX!!!!!"
                );
            }
        } else {
            // Indirect call (memory operand)
            let target_ea = ctx.ea(&operands[0]);
            cpp!(
                ctx.out,
                "
                val_uint32_0 = X86_LOAD_U32({target_ea});
                ctx.SubEsp(1); //Adjust stack pointer for call instruction
            "
            );
            ctx.x87_local_to_ctx();
            cpp!(ctx.out, "X86_CALL_INDIRECT_FUNC(val_uint32_0);");
            ctx.x87_ctx_to_local();
        }
    } else if register(&operands[0]).is_some() {
        let reg = get_x86_reg(register(&operands[0]).unwrap());
        cpp!(ctx.out, "val_uint32_0 = {reg};");

        // Register indirect (call eax)
        if ctx.function_state.has_register_import
            && let Some((low, high)) = ctx.platform.register_call_window()
        {
            let import_addr = ctx.function_state.register_import_thunk;
            cpp!(
                ctx.out,
                "
                if (val_uint32_0 >=  0x{low:X} && val_uint32_0 <= 0x{high:X}) {{
                    val_uint32_0 = 0x{import_addr:X};
                }}
            "
            );
        }

        cpp!(
            ctx.out,
            "ctx.SubEsp(1); //Adjust stack pointer for call instruction"
        );
        ctx.x87_local_to_ctx();
        cpp!(ctx.out, "X86_CALL_INDIRECT_FUNC(val_uint32_0);");
        ctx.x87_ctx_to_local();
    } else {
        ctx.missing_form();
    }
}
