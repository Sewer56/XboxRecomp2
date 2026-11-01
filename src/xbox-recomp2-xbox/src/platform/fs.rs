//! Handles `fs:[...]` accesses.
//!
//! On Xbox, `fs` points to the kernel's per-CPU data (KPCR).
//!
//! In this recompiler, we keep that in `ctx.kpcr_ptr`.
//! So `fs:[0x20]` becomes `ctx.kpcr_ptr + 0x20`.

use xbox_recomp2::platform::{MemoryRoute, operand_size_name};
use zydis::Mnemonic;

/// Routes one `fs:[address]` access.
pub(super) fn route(insn: Mnemonic, address: u32, size: u16, is_write: bool) -> MemoryRoute {
    match insn {
        Mnemonic::MOV => MemoryRoute::Statement(mov_fs_template(address, size, is_write)),
        Mnemonic::PUSH => MemoryRoute::Statement(format!(
            "\tX86_PUSH(ctx, base, X86_LOAD_{}(ctx.kpcr_ptr + 0x{address:X}));",
            operand_size_name(size)
        )),
        Mnemonic::POP => MemoryRoute::Statement(format!(
            "\t//POP FS:[0x{address:X}] (skipped)\n\t(void)X86_POP(ctx, base);  // Discard value"
        )),
        Mnemonic::MOVZX => MemoryRoute::Expr(format!(
            "X86_LOAD_{}(ctx.kpcr_ptr + 0x{address:X})",
            operand_size_name(size)
        )),
        Mnemonic::MOVSX => MemoryRoute::Statement(movsx_fs_template(address, size)),
        _ => MemoryRoute::Plain,
    }
}

/// Builds `mov` to or from `fs:[offset]`.
fn mov_fs_template(offset: u32, size: u16, is_write: bool) -> String {
    let size_name = operand_size_name(size);
    let kpcr_access = if is_write {
        format!("X86_STORE_{size_name}(ctx.kpcr_ptr + 0x{offset:X}")
    } else {
        format!("X86_LOAD_{size_name}(ctx.kpcr_ptr + 0x{offset:X})")
    };

    match offset {
        0x00 | 0x1C | 0x20 | 0x24 | 0x28 => {
            if is_write {
                format!("\t{kpcr_access}, {{reg}});  // write to FS:[0x{offset:X}]")
            } else {
                format!("\t{{reg}} = {kpcr_access};  // read from FS:[0x{offset:X}]")
            }
        }
        0x04 => {
            if is_write {
                format!("\t{kpcr_access}, {{reg}});  // write to FS:[4] (TLS mid-array)")
            } else {
                format!("\t{{reg}} = {kpcr_access};  // read from FS:[4] (TLS mid-array)")
            }
        }
        _ => {
            if is_write {
                format!("\t{kpcr_access}, {{reg}});  // WARNING: Unknown FS write at 0x{offset:X}")
            } else {
                format!("\t{{reg}} = {kpcr_access};  // WARNING: Unknown FS read at 0x{offset:X}")
            }
        }
    }
}

/// Builds `movsx` from `fs:[offset]`.
fn movsx_fs_template(offset: u32, size: u16) -> String {
    match size {
        8 => format!(
            "\tint8_t tmp = (int8_t)X86_LOAD_U8(ctx.kpcr_ptr + 0x{offset:X});\n\t{{reg}} = (int32_t)tmp;"
        ),
        16 => format!(
            "\tint16_t tmp = (int16_t)X86_LOAD_U16(ctx.kpcr_ptr + 0x{offset:X});\n\t{{reg}} = (int32_t)tmp;"
        ),
        _ => String::new(),
    }
}
