use super::super::prelude::*;
use super::dword_unpack;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    dword_unpack::emit(
        ctx,
        "X86_LOAD_U32",
        "X86_PUNPCKLDQ(val_uint64_0, val_uint64_1)",
    );
}
