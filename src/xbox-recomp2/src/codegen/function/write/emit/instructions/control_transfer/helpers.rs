//! Code shared by calls, jumps and conditional jumps.

use super::super::prelude::*;

pub(in super::super) fn print_conditional_branch(ctx: &mut Emitter<'_>, not_: bool, cond: &str) {
    print_conditional_branch_with(ctx, not_, cond, None);
}

/// Writes a conditional jump, running `before` first on the taken path.
pub(in super::super) fn print_conditional_branch_with(
    ctx: &mut Emitter<'_>,
    not_: bool,
    cond: &str,
    before: Option<&str>,
) {
    let operands = ctx.insn.operands();
    let mut target = ctx.base.wrapping_add(ctx.insn.length() as u32);

    if let Some(imm) = immediate(&operands[0]) {
        target = target.wrapping_add(imm.value as i64 as u32); // Signed displacement
    } else if let Some(mem) = memory(&operands[0])
        && mem.disp.displacement != 0
    {
        target = target.wrapping_add(mem.disp.displacement as u32);
    }
    target = ctx.masked_branch_target(target);
    let negation = if not_ { "!" } else { "" };

    if target.is_zero() {
        if ctx.config.debug {
            let base = ctx.base.get();
            ctx.out.line(&format!(
                "printf(\"WARNING: Branch target resolves to 0x0 (possible analysis error) - {base:X}\n\");"
            ));
        }
        cpp!(
            ctx.out,
            "
            // WARNING: Branch target resolves to 0x0 (possible analysis error)
            if ({negation}{cond}) goto invalid_target;
        "
        );
    } else if ctx.is_local_target(target) {
        // Internal jump
        if let Some(before) = before {
            cpp!(ctx.out, "if ({negation}{cond}) {{");
            ctx.out.indent();
            cpp!(ctx.out, "{before}");
            cpp!(ctx.out, "goto loc_{target:X};");
            ctx.out.dedent();
            cpp!(ctx.out, "}}");
        } else {
            cpp!(ctx.out, "if ({negation}{cond}) goto loc_{target:X};");
        }
    } else {
        // External function call
        cpp!(ctx.out, "if ({negation}{cond}) {{");
        ctx.out.indent();
        if let Some(before) = before {
            cpp!(ctx.out, "{before}");
        }
        ctx.x87_local_to_ctx();
        print_function_call_addr(ctx, target, false);
        cpp!(ctx.out, "return;");
        ctx.out.dedent();
        cpp!(ctx.out, "}}");
    }
}

pub(super) fn print_function_call_addr(
    ctx: &mut Emitter<'_>,
    address: GuestAddress32,
    fastcall: bool,
) -> bool {
    let symbols = ctx.symbols;
    let symbol = symbols
        .find(u64::from(address))
        .filter(|symbol| symbol.kind == SymbolType::Function);

    if let Some(symbol) = symbol {
        let name = &symbol.name;
        if fastcall {
            cpp!(ctx.out, "X86_EXTERN_FUNC({name}{FASTCALL_SUFFIX});");
            cpp!(ctx.out, "{name}{FASTCALL_SUFFIX}(ctx, base);");
        } else {
            cpp!(ctx.out, "{name}(ctx, base);");
        }
        return true;
    } else {
        if ctx.config.debug {
            let base = ctx.base.get();
            cpp!(
                ctx.out,
                "printf(\"WARNING UNKNOWN FUNCTION REACHED {address:X} at: {base:X}\\n\");"
            );
        }
        cpp!(ctx.out, "// ERROR: Unknown function at 0x{address:X}");
    }

    false
}
