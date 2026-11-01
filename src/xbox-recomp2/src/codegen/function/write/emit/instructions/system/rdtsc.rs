use super::super::prelude::*;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    if ctx.config.debug {
        let base = ctx.base.get();
        cpp!(
            ctx.out,
            "printf(\"RDTSC detected! Require patching to handle Xbox speed!!! {base:X}\\n\");"
        );
    }

    cpp!(ctx.out, "X86_RDTSC(ctx);");
}
