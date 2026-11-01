//! Integer math and compares.

pub(super) mod aaa;
pub(super) mod aad;
pub(super) mod aas;
pub(super) mod adc;
pub(super) mod add;
pub(super) mod cdq;
pub(super) mod cmp;
pub(super) mod cwde;
pub(super) mod dec;
pub(super) mod div;
pub(super) mod idiv;
pub(super) mod imul;
pub(super) mod inc;
pub(super) mod mul;
pub(super) mod neg;
pub(super) mod sbb;
pub(super) mod sub;
pub(super) mod xadd;

use super::data_transfer::{merges_into_parent, register_assignment};
use super::prelude::{Emitter, Register, cpp, get_x86_reg};

/// Writes a flag-setting helper call whose result goes to register `dst`.
fn write_register_op(ctx: &mut Emitter<'_>, dst: Register, helper: &str, operands: &str) {
    let statement = register_op_statement(dst, helper, operands, &ctx.flag_template(false));
    cpp!(ctx.out, "{statement};");
}

/// Builds a flag-setting helper call whose result goes to register `dst`.
/// 8-bit and 16-bit destinations use `_VAL`, like [`register_assignment`].
///
/// # Arguments
///
/// - `dst`: destination register, also the helper's first operand.
/// - `helper`: helper name with its width, such as `X86_ADD16`.
/// - `operands`: the remaining operands, each followed by `, `.
///
/// # Returns
///
/// The statement without its `;`, for example:
///
/// ```text
/// ctx.eax.u32 = (ctx.eax.u32 & 0xFFFF0000u) | (uint16_t)(X86_ADD16_VAL(ctx.eax.u16, 0x10, ctx))
/// ```
fn register_op_statement(dst: Register, helper: &str, operands: &str, flags: &str) -> String {
    let reg = get_x86_reg(dst);
    if merges_into_parent(dst) {
        register_assignment(dst, &format!("{helper}_VAL{flags}({reg}, {operands}ctx)"))
    } else {
        format!("{helper}{flags}({reg}, {operands}ctx)")
    }
}
