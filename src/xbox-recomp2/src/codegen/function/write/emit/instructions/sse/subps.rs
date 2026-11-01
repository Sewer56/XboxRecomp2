use super::super::prelude::*;
use super::helpers::float_s;

pub(in super::super) fn emit(ctx: &mut Emitter<'_>) {
    float_s(ctx, "-");
}
