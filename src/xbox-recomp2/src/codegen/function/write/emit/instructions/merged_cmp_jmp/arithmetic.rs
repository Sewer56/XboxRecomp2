//! Writes merged math like `sub eax, 1`.

use super::super::data_transfer::register_assignment;
use super::super::prelude::*;
use super::{IntegerCompare, constant, written_later};
use crate::codegen::function::model::{Condition, MergedCmpJmp, MergedInstructionKind};
use crate::decode::MachineMode;

/// Writes a merged math instruction, like `sub eax, ebx`.
///
/// # Returns
///
/// Its operands and result, for the jumps and selects to test.
///
/// # Examples
///
/// `sub eax, ebx; jz`:
///
/// ```text
/// ctx.eax.u32 -= ctx.ebx.u32;
/// ```
///
/// `sub eax, ebx; jb`:
///
/// ```text
/// const uint32_t old = ctx.eax.u32; // <- jb compares eax from before the sub
/// ctx.eax.u32 = old - ctx.ebx.u32;
/// ```
///
/// `dec dword ptr [esi]; jz`:
///
/// ```text
/// const uint32_t result = X86_LOAD_U32(ctx.esi.u32) - 1;
/// X86_STORE_U32(ctx.esi.u32, result);
/// ```
pub(super) fn write_compare(ctx: &mut Emitter<'_>, merged: &MergedCmpJmp) -> IntegerCompare {
    let mnemonic = ctx.insn.mnemonic();
    let operands = ctx.insn.operands();
    let destination = &operands[0];
    let bits = destination.size;
    let kind = operand_type_name(bits);
    let suffix = operand_size_name(bits);
    let needs_old = needs_old(merged, mnemonic);

    // const uint32_t source = X86_LOAD_U32(ctx.esi.u32);
    let mut source = source(ctx);
    if needs_old && is_binary(mnemonic) && source_may_change(merged, destination, &operands[1]) {
        cpp!(ctx.out, "const {kind} source = {source};");
        source = "source".to_owned();
    }

    // const uint32_t old = ctx.eax.u32;
    let mut old = if memory(destination).is_some() {
        format!("X86_LOAD_{suffix}({})", ea(destination))
    } else {
        format_operand(destination, suffix)
    };
    if needs_old {
        cpp!(ctx.out, "const {kind} old = {old};");
        old = "old".to_owned();
    }

    // ctx.eax.u32 = old - source;
    let value = value(mnemonic, &old, &source, bits);
    let result = write_result(ctx, merged, &old, &source, &value);
    IntegerCompare {
        mnemonic,
        lhs: old,
        rhs: source,
        result,
        bits,
    }
}

/// The second value, like `ctx.ebx.u32`, `0x1u` or a shift count.
/// Empty for `neg`.
fn source(ctx: &Emitter<'_>) -> String {
    let operands = ctx.insn.operands();
    match ctx.insn.mnemonic() {
        Mnemonic::INC | Mnemonic::DEC => "1".to_owned(),
        Mnemonic::NEG => String::new(),
        Mnemonic::SHL | Mnemonic::SHR | Mnemonic::SAR => {
            let count = immediate(&operands[1]).map_or(0, |imm| imm.value & 31);
            count.to_string()
        }
        _ => match immediate(&operands[1]) {
            Some(imm) => constant(imm.value, operands[0].size),
            None => source_operand(ctx, &operands[1]),
        },
    }
}

/// Reads a math instruction's register or memory source.
///
/// For `and`, `or` and `xor`, the platform may replace a `ds:` read.
///
/// # Examples
///
/// ```text
/// sub eax, ebx                ; ctx.ebx.u32
/// sub eax, [esi]              ; X86_LOAD_U32(ctx.esi.u32)
/// xor eax, ds:[import]        ; XboxSignatureKey[0]  <- Xbox data import
/// xor eax, ds:[import+esi*4]  ; XboxSignatureKey[ctx.esi.u32]
/// ```
fn source_operand(ctx: &Emitter<'_>, operand: &DecodedOperand) -> String {
    platform_source(ctx, operand)
        .unwrap_or_else(|| format_operand(operand, operand_size_name(operand.size)))
}

/// The platform's C++ for a `ds:` source of `and`, `or` or `xor`.
///
/// [`None`] if the platform doesn't replace it.
fn platform_source(ctx: &Emitter<'_>, operand: &DecodedOperand) -> Option<String> {
    let is_logic = matches!(
        ctx.insn.mnemonic(),
        Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR
    );

    if !is_logic {
        return None;
    }

    let mem = memory(operand).filter(|mem| mem.segment == Register::DS)?;
    let address = mem.disp.displacement as u32;
    match ctx.route(Register::DS, address, operand.size, false) {
        // XboxSignatureKey[{reg}] -> XboxSignatureKey[ctx.esi.u32]
        MemoryRoute::Expr(expr) => {
            Some(expr.replace("{reg}", &array_subscript(operand, operand.size)))
        }
        _ => None,
    }
}

/// Whether `source` may change before a jump or select uses it.
///
/// # Examples
///
/// ```text
/// sub eax, [esi]  ; memory: yes
/// ```
///
/// ```text
/// sub eax, eax    ; the sub writes eax: yes
/// ```
///
/// ```text
/// sub eax, ebx
/// mov ebx, 0      ; writes ebx: yes
/// ```
///
/// ```text
/// sub eax, ebx    ; nothing writes ebx: no
/// jb  below
/// ```
fn source_may_change(
    merged: &MergedCmpJmp,
    destination: &DecodedOperand,
    source: &DecodedOperand,
) -> bool {
    // sub eax, [esi]
    if memory(source).is_some() {
        return true;
    }

    // sub eax, 1
    let Some(reg) = register(source) else {
        return false;
    };

    // sub eax, eax
    let math_writes_it = overlaps(destination, reg);

    // sub eax, ebx; mov ebx, 0
    let later_writes_it = written_later(merged, reg);

    math_writes_it || later_writes_it
}

/// Whether register `reg` shares bits with `operand`, like `al` and `eax`.
fn overlaps(operand: &DecodedOperand, reg: Register) -> bool {
    let parent = |reg: Register| reg.largest_enclosing(MachineMode::LEGACY_32);
    register(operand).is_some_and(|other| parent(other) == parent(reg))
}

/// Writes `value` to the destination.
///
/// # Returns
///
/// Where the jumps and selects read the result,
/// like `ctx.ecx.u32` or `result`.
///
/// # Examples
///
/// `dec ecx; jz`:
///
/// ```text
/// ctx.ecx.u32 -= 1; // <- returns ctx.ecx.u32
/// ```
///
/// `dec dword ptr [esi]; jz`:
///
/// ```text
/// const uint32_t result = X86_LOAD_U32(ctx.esi.u32) - 1;
/// X86_STORE_U32(ctx.esi.u32, result);
/// ```
///
/// `dec ecx; mov ecx, 0; jz`:
///
/// ```text
/// ctx.ecx.u32 -= 1;
/// const uint32_t result = ctx.ecx.u32; // <- mov overwrites ecx
/// ```
fn write_result(
    ctx: &mut Emitter<'_>,
    merged: &MergedCmpJmp,
    old: &str,
    source: &str,
    value: &str,
) -> String {
    let destination = &ctx.insn.operands()[0];
    let kind = operand_type_name(destination.size);
    let suffix = operand_size_name(destination.size);

    // const uint32_t result = X86_LOAD_U32(ctx.esi.u32) - 1;
    // X86_STORE_U32(ctx.esi.u32, result);
    if memory(destination).is_some() {
        cpp!(ctx.out, "const {kind} result = {value};");
        cpp!(ctx.out, "X86_STORE_{suffix}({}, result);", ea(destination));
        return "result".to_owned();
    }

    // ctx.ecx.u32 -= 1;
    let reg = register(destination).expect("register or memory destination");
    write_register(ctx, reg, old, source, value);
    let written = format_operand(destination, suffix);
    if !written_later(merged, reg) {
        return written;
    }

    // const uint32_t result = ctx.ecx.u32;
    cpp!(ctx.out, "const {kind} result = {written};");
    "result".to_owned()
}

/// Writes `value` to register `reg`.
///
/// # Examples
///
/// `dec ecx`:
///
/// ```text
/// ctx.ecx.u32 -= 1;
/// ```
///
/// `sub eax, ebx`, with `eax` copied to `old`:
///
/// ```text
/// ctx.eax.u32 = old - ctx.ebx.u32;
/// ```
///
/// `dec al`:
///
/// ```text
/// ctx.eax.u32 = (ctx.eax.u32 & 0xFFFFFF00u) | (uint8_t)(ctx.eax.u8_lo - 1);
/// ```
fn write_register(ctx: &mut Emitter<'_>, reg: Register, old: &str, source: &str, value: &str) {
    // ctx.ecx.u32 -= 1;
    let target = get_x86_reg(reg);
    let is_full_register = reg.class() == RegisterClass::GPR32; // <- al, ax need masking
    let reads_itself = old == target; // <- not copied to `old`
    if let Some(operator) = operator(ctx.insn.mnemonic())
        && is_full_register
        && reads_itself
    {
        cpp!(ctx.out, "{target} {operator}= {source};");
        return;
    }

    // ctx.eax.u32 = old - ctx.ebx.u32;
    let assignment = register_assignment(reg, value);
    cpp!(ctx.out, "{assignment};");
}

/// The C++ for the math's result.
///
/// # Examples
///
/// ```text
/// sub eax, ebx    ; old - ctx.ebx.u32
/// ```
///
/// ```text
/// neg eax         ; 0 - old
/// ```
///
/// ```text
/// sar eax, 4      ; static_cast<int32_t>(old) >> 4
/// ```
///
/// ```text
/// shl al, 4       ; static_cast<uint32_t>(old) << 4
/// ```
///
/// # Remarks
///
/// May be wider than `bits`.
/// The caller stores it at the right width, which drops the extra bits.
fn value(mnemonic: Mnemonic, old: &str, source: &str, bits: u16) -> String {
    match mnemonic {
        // shl al, 4
        // C++ widens `al` to `int`, which `<< 31` overflows.
        Mnemonic::SHL if bits < 32 => format!("static_cast<uint32_t>({old}) << {source}"),

        // neg eax
        Mnemonic::NEG => format!("0 - {old}"),

        // sar eax, 4
        // C++ shifts signed values arithmetically, like `sar`.
        Mnemonic::SAR => format!("static_cast<int{bits}_t>({old}) >> {source}"),

        // sub eax, ebx
        _ => {
            let operator = operator(mnemonic).expect("merging accepts only these");
            format!("{old} {operator} {source}")
        }
    }
}

/// The C++ operator for `mnemonic`, like `-` for `sub` and `dec`.
fn operator(mnemonic: Mnemonic) -> Option<&'static str> {
    Some(match mnemonic {
        Mnemonic::ADD | Mnemonic::INC => "+",
        Mnemonic::SUB | Mnemonic::DEC => "-",
        Mnemonic::AND => "&",
        Mnemonic::OR => "|",
        Mnemonic::XOR => "^",
        Mnemonic::SHL => "<<",
        Mnemonic::SHR => ">>",
        _ => return None,
    })
}

/// Whether `mnemonic` takes a source operand.
fn is_binary(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::ADD | Mnemonic::SUB | Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR
    )
}

/// Whether we need the destination's value from before the math.
///
/// # Examples
///
/// ```text
/// sub eax, ebx
/// jb  below       ; old eax < ebx: yes
/// ```
///
/// ```text
/// add eax, ebx
/// jb  wrapped     ; eax < old eax: yes
/// ```
///
/// ```text
/// dec ecx
/// jz  done        ; writes ZF when taken: yes
/// ```
///
/// ```text
/// xor eax, ebx
/// jz  done        ; writes ZF when taken: no, logic flags need only the result
/// ```
///
/// ```text
/// sub eax, ebx
/// jz  zero        ; eax == 0: no
/// ```
fn needs_old(merged: &MergedCmpJmp, mnemonic: Mnemonic) -> bool {
    // sub eax, ebx; jb
    // add eax, ebx; jb
    let tests_old = merged
        .instructions
        .iter()
        .filter(|insn| insn.kind != MergedInstructionKind::Plain)
        .any(|insn| {
            let condition = MergedCmpJmp::condition(mnemonic, insn.guest.insn.mnemonic());
            matches!(
                condition,
                Some(Condition::Compare(..) | Condition::Carry(_))
            )
        });

    if tests_old {
        return true;
    }

    // xor eax, ebx
    let is_logic = matches!(mnemonic, Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR);
    if is_logic {
        return false;
    }

    // dec ecx; jz
    let jump_writes_flags = merged
        .instructions
        .iter()
        .any(|insn| matches!(insn.kind, MergedInstructionKind::Jump(flags) if !flags.is_empty()));

    jump_writes_flags || !merged.end_flags.is_empty()
}
