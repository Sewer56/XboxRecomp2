use super::super::prelude::*;
use super::word_unpack::{self, WordUnpack};

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    word_unpack::emit(
        ctx,
        &WordUnpack {
            mmx_helper: "X86_PUNPCKHWD",
            mmx_load: "X86_LOAD_U64",
            xmm_intrinsic: "_mm_unpackhi_epi16",
        },
    );
}
