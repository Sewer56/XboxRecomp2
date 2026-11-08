use super::super::prelude::*;
use super::helpers::print_function_call_addr;

const SWITCH_INDEX_FAILURE: &str = r#"		fprintf(stderr, "Unhandled switch index at 0x{{address}}: %u ({{count}} entries); check [[switch]] configuration.\n", static_cast<unsigned>(ctx.{{register}}.u32));
		fflush(stderr);
		std::abort();
"#;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    let operands = ctx.insn.operands();
    if operands[0].size == 48 || matches!(operands[0].kind, DecodedOperandKind::Ptr(_)) {
        return ctx.missing_form();
    }

    if memory(&operands[0]).is_some() {
        if let Some(table) = ctx.switch_table {
            //Generate a switch statement for the jump table
            let index = table.register;
            cpp!(ctx.out, "switch (ctx.{index}.u32) {{");

            for (i, &label) in table.labels.iter().enumerate() {
                cpp!(ctx.out, "case {i}:");
                ctx.out.indent();
                let label = GuestAddress32::from(label);
                if ctx.is_local_target(label) {
                    cpp!(ctx.out, "goto loc_{label:X};");
                } else if print_function_call_addr(ctx, label, false) {
                    cpp!(ctx.out, "return;");
                } else {
                    switch_case_outside(ctx, label);
                }
                ctx.out.dedent();
            }

            cpp!(ctx.out, "default:");
            if ctx.config.debug {
                ctx.out.raw(&render(
                    SWITCH_INDEX_FAILURE,
                    &[
                        ("address", &format!("{:X}", ctx.base.get())),
                        ("register", &index.to_string()),
                        ("count", &table.labels.len().to_string()),
                    ],
                ));
            } else {
                ctx.out.indent();
                cpp!(ctx.out, "X86_UNREACHABLE();");
                ctx.out.dedent();
            }
            cpp!(ctx.out, "}}");
        } else {
            //Handle regular indirect jump
            // Handle regular indirect jump dynamically
            let target =
                GuestAddress32::from(memory(&operands[0]).unwrap().disp.displacement as u32);
            if ctx.imports.resolve_thunk(target.get().into()).is_some() {
                print_function_call_addr(ctx, target, false);
            } else {
                let target_ea = ctx.ea(&operands[0]);
                cpp!(
                    ctx.out,
                    "X86_CALL_INDIRECT_FUNC(X86_LOAD_U32({target_ea}));"
                );
            }
            cpp!(ctx.out, "return;");
        }
    } else if immediate(&operands[0]).is_some() {
        // Handle direct jumps (JMP imm)
        let mut target = ctx
            .base
            .wrapping_add(ctx.insn.length() as u32)
            .wrapping_add(immediate(&operands[0]).unwrap().value as i64 as u32);
        target = ctx.masked_branch_target(target);

        if !ctx.is_local_target(target) {
            // Target is outside the current function: call the function
            print_function_call_addr(ctx, target, false);
            cpp!(ctx.out, "return;");
        } else {
            //Target is within the current function: generate a goto statement
            cpp!(ctx.out, "goto loc_{target:X};");
        }
    } else if let Some(reg) = register(&operands[0]) {
        let reg = get_x86_reg(reg);
        cpp!(
            ctx.out,
            "
            X86_CALL_INDIRECT_FUNC({reg});
            return;
        "
        );
    } else {
        ctx.missing_form();
    }
}

fn switch_case_outside(ctx: &mut Emitter<'_>, label: GuestAddress32) {
    ctx.out.indent();
    cpp!(ctx.out, "// ERROR: 0x{label:X}");
    println!(
        "ERROR: Switch case at {:X} is trying to jump outside function: {:X}",
        ctx.base.get(),
        label
    );
    cpp!(ctx.out, "return;");
    ctx.out.dedent();
}
