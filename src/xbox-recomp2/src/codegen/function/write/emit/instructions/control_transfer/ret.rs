//! Writes the stack cleanup selected for a return.

use super::super::prelude::*;
use crate::codegen::function::model::Return;

pub(in crate::codegen::function::write) fn emit(ctx: &mut Emitter<'_>, ret: &Return) {
    // Stack to free:
    // - The arguments.
    // - The return address, if the caller pushed one.
    let return_slot_bytes = if ret.omit_return_slot { 0 } else { 4 };
    let bytes = u32::from(ret.argument_bytes) + return_slot_bytes;
    if bytes != 0 {
        cpp!(
            ctx.out,
            "ctx.esp.u32 += 0x{bytes:X};  //Adjust esp for return instruction"
        );
    }
    cpp!(ctx.out, "return;");
}
