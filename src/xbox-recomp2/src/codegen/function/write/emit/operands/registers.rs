//! Gets you the C++ text for an x86 register.
//!
//! The generated C++ keeps all CPU registers in a struct called `ctx`.
//! So `eax` becomes `ctx.eax.u32`, and `al` becomes `ctx.eax.u8_lo`.

use super::MODE;
use zydis::Register;

/// Returns the C++ text for a normal register, like `ctx.eax.u16` for `ax`.
///
/// # Panics
///
/// Panics for registers it doesn't know, like float and vector ones.
/// Use [`get_x86_reg_float`] for those.
pub fn get_x86_reg(reg: Register) -> String {
    if reg == Register::EFLAGS {
        return "ctx.eflags.packed()".to_string();
    }

    // Segment registers read as ctx.<name>.
    let raw = reg as u16;
    if (Register::ES as u16..=Register::GS as u16).contains(&raw) {
        const SEGMENT_NAMES: [&str; 6] = ["es", "cs", "ss", "ds", "fs", "gs"];
        let index = usize::from(raw - Register::ES as u16);
        return format!("ctx.{}", SEGMENT_NAMES[index]);
    }

    let parent = reg.largest_enclosing(MODE);
    let name = match parent {
        Register::ESP => "esp",
        Register::EBP => "ebp",
        Register::ESI => "esi",
        Register::EDI => "edi",
        Register::EAX => "eax",
        Register::EBX => "ebx",
        Register::ECX => "ecx",
        Register::EDX => "edx",
        _ => panic!(
            "Unsupported base register: {}",
            parent.static_string().unwrap_or_default()
        ),
    };

    match reg.width(MODE) {
        8 => match reg {
            Register::AL | Register::BL | Register::CL | Register::DL => {
                format!("ctx.{name}.u8_lo")
            }
            Register::AH | Register::BH | Register::CH | Register::DH => {
                format!("ctx.{name}.u8_hi")
            }
            _ => panic!("Unsupported 8-bit subregister"),
        },
        16 => format!("ctx.{name}.u16"),
        32 => format!("ctx.{name}.u32"),
        _ => panic!("Unsupported size"),
    }
}

/// Returns the C++ text for a float or vector register.
///
/// # Arguments
///
/// - `reg`: the register.
/// - `fpu`: C++ name of the x87 state, from [`Emitter::fpu`].
/// - `mmx`: C++ name of the MMX registers, from [`Emitter::mmx`].
///
/// # Returns
///
/// The register's C++ text, like `ctx.xmm[2].reg` for `xmm2` or
/// `ctx.fpu.st1()` for `st1`.
///
/// # Panics
///
/// Panics for any other register.
///
/// [`Emitter::fpu`]: super::super::Emitter::fpu
/// [`Emitter::mmx`]: super::super::Emitter::mmx
#[inline]
pub fn get_x86_reg_float(reg: Register, fpu: &str, mmx: &str) -> String {
    if is_mm0_to_7(reg) {
        return format!("{mmx}[{}]", mmx_index(reg));
    }
    if is_xmm0_to_7(reg) {
        return format!("ctx.xmm[{}].reg", xmm_index(reg));
    }
    if is_st0_to_7(reg) {
        return format!("{fpu}.st{}()", reg as u16 - Register::ST0 as u16);
    }
    if reg == Register::MXCSR {
        return "ctx.mxcsr".to_string();
    }
    panic!("Unsupported FP/SIMD register");
}

/// Returns the number of an MMX register, like `3` for `mm3`.
///
/// Doesn't check that `reg` is actually an MMX register.
#[inline]
pub fn mmx_index(reg: Register) -> i64 {
    reg as i64 - Register::MM0 as i64
}

/// Returns the number of an x87 float register, like `3` for `st3`.
///
/// Doesn't check that `reg` is actually an x87 register.
#[inline]
pub fn st_index(reg: Register) -> i64 {
    reg as i64 - Register::ST0 as i64
}

/// Returns the number of an XMM register, like `3` for `xmm3`.
///
/// Doesn't check that `reg` is actually an XMM register.
#[inline]
pub fn xmm_index(reg: Register) -> i64 {
    reg as i64 - Register::XMM0 as i64
}

/// Returns whether `reg` is `mm0` through `mm7`.
#[inline]
pub fn is_mm0_to_7(reg: Register) -> bool {
    (Register::MM0 as u16..=Register::MM7 as u16).contains(&(reg as u16))
}

/// Returns whether `reg` is `st0` through `st7`.
#[inline]
pub fn is_st0_to_7(reg: Register) -> bool {
    (Register::ST0 as u16..=Register::ST7 as u16).contains(&(reg as u16))
}

/// Returns whether `reg` is `xmm0` through `xmm7`.
#[inline]
pub fn is_xmm0_to_7(reg: Register) -> bool {
    (Register::XMM0 as u16..=Register::XMM7 as u16).contains(&(reg as u16))
}

/// Returns the full 32-bit C++ text for a register in a memory address.
///
/// For example, both `si` and `esi` give `ctx.esi.u32`.
///
/// # Panics
///
/// Panics unless `reg` is part of `eax`, `ebx`, `ecx`, `edx`, `esi`, `edi`,
/// `ebp` or `esp`.
#[inline]
pub(super) fn get_addr_reg(reg: Register) -> String {
    let text = match reg.largest_enclosing(MODE) {
        Register::EAX => "ctx.eax.u32",
        Register::EBX => "ctx.ebx.u32",
        Register::ECX => "ctx.ecx.u32",
        Register::EDX => "ctx.edx.u32",
        Register::ESI => "ctx.esi.u32",
        Register::EDI => "ctx.edi.u32",
        Register::EBP => "ctx.ebp.u32",
        Register::ESP => "ctx.esp.u32",
        _ => panic!("Unsupported address register"),
    };
    text.to_string()
}
