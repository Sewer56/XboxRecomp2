//! A `cmp` or similar, merged with the jumps that read its flags.
//!
//! This lets us test the values directly and skip writing `eflags`.
//!
//! For example, this:
//!
//! ```text
//! cmp eax, 0x20   ; sets flags
//! jz  equal       ; reads ZF
//! ```
//!
//! becomes:
//!
//! ```text
//! if (ctx.eax.u32 == 0x20u) goto equal;
//! ```
//!
//! SETcc and CMOVcc merge the same way.
//!
//! # Math instructions
//!
//! Math like `sub` merges too.
//!
//! For example:
//!
//! ```text
//! sub eax, ebx    ; sets flags from eax - ebx
//! jb  below       ; jumps if the old eax < ebx
//! ```
//!
//! becomes:
//!
//! ```text
//! const uint32_t old = ctx.eax.u32;
//! ctx.eax.u32 = old - ctx.ebx.u32;
//! if (old < ctx.ebx.u32) goto below;
//! ```
//!
//! Because `sub` also writes flags, later read by `cmp`.

use super::super::GuestOp;
use crate::decode::{
    BranchType, CpuFlag, DecodedOperand, Insn, InstructionAttributes, InstructionCategory,
    MachineMode, Mnemonic, Register, RegisterClass, immediate, memory, register,
};

/// The C++ condition for a merged jump, SETcc or CMOVcc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Condition {
    /// Compares the two operands, after `cmp` or `sub`.
    ///
    /// ```text
    /// cmp eax, ebx
    /// jb  below       ; Compare("<", false) -> if (eax < ebx) goto below;
    /// jl  less        ; Compare("<", true)  -> if ((int32_t)eax < (int32_t)ebx) goto less;
    /// ```
    ///
    /// `true` for signed, `false` for unsigned.
    Compare(&'static str, bool),

    /// Compares the result with 0.
    ///
    /// ```text
    /// dec ecx
    /// jz  done        ; CompareToZero("==", false) -> if (ecx == 0) goto done;
    ///
    /// test eax, eax
    /// jl  negative    ; CompareToZero("<", true)   -> if ((int32_t)eax < 0) goto negative;
    /// ```
    ///
    /// `true` for signed, `false` for unsigned.
    CompareToZero(&'static str, bool),

    /// Checks whether the result is negative.
    ///
    /// ```text
    /// cmp eax, ebx
    /// js  negative    ; Sign(true) -> if ((int32_t)(eax - ebx) < 0) goto negative;
    /// ```
    ///
    /// `true` for `js`, `false` for `jns`.
    Sign(bool),

    /// Checks whether the result's low byte has an even number of 1 bits.
    ///
    /// ```text
    /// test al, al
    /// jp   even       ; Parity(true) -> if (__builtin_parity(al) == 0) goto even;
    /// ```
    ///
    /// `true` for `jp`, `false` for `jnp`.
    Parity(bool),

    /// Checks whether an unsigned `add` overflowed.
    ///
    /// ```text
    /// add eax, ebx
    /// jb  overflow    ; Carry(true) -> if (eax < old) goto overflow;
    /// ```
    ///
    /// `old` is `eax` before the `add`.
    /// `true` for `jb`, `false` for `jnb`.
    Carry(bool),

    /// Checks flags directly, after `sahf` or a float compare.
    ///
    /// ```text
    /// sahf
    /// jz  equal       ; Flags(ZF, true) -> if ((ah & ZF) != 0) goto equal;
    /// ```
    ///
    /// `true` if any flag is set, `false` if none are.
    ///
    /// A float compare sets CF, ZF and PF when either value is NaN.
    Flags(CpuFlag, bool),
}

/// A `cmp` or similar, merged with the jumps after it.
#[derive(Clone, Debug)]
pub(crate) struct MergedCmpJmp {
    /// The instruction that sets the flags, like `cmp` or `dec`.
    pub compare: GuestOp,

    /// An `fstp st0` right after an FCOMIP or FUCOMIP.
    ///
    /// See markdown doc for special case that's handled here.
    pub x87_pop: Option<GuestOp>,

    /// The instructions after the compare (and x87 pop), in guest order.
    /// Ends with instruction that reads the flag(s).
    pub instructions: Vec<MergedInstruction>,

    /// Flags to write after the last instruction, for the code that follows.
    ///
    /// ```text
    /// cmp eax, ebx
    /// jz  equal       ; last instruction
    /// adc ecx, 0      ; reads CF, so `end_flags` is CF
    /// ```
    ///
    /// Empty if later code reads none of the flags the compare changes.
    pub end_flags: CpuFlag,
}

/// One instruction after the compare in a [`MergedCmpJmp`].
#[derive(Clone, Debug)]
pub(crate) struct MergedInstruction {
    pub guest: GuestOp,
    pub kind: MergedInstructionKind,
}

/// How a [`MergedInstruction`] is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MergedInstructionKind {
    /// Doesn't touch flags, so it's written as usual.
    Plain,

    /// A SETcc or CMOVcc.
    Select,

    /// A jump, with the flags needed when it's taken.
    Jump(CpuFlag),
}

impl MergedCmpJmp {
    /// Whether `compare` has a form the merged writer supports.
    pub(in crate::codegen) fn accepts(compare: &GuestOp) -> bool {
        let insn = &compare.insn;
        if compare.last_in_function || insn.instruction().address_width != 32 {
            return false;
        }

        match insn.mnemonic() {
            Mnemonic::CMP | Mnemonic::TEST => accepts_integer(insn),
            Mnemonic::COMISS | Mnemonic::UCOMISS => accepts_sse(insn),
            Mnemonic::FCOMIP | Mnemonic::FUCOMIP => accepts_x87(insn),
            Mnemonic::SAHF => true,
            mnemonic if is_arithmetic(mnemonic) => accepts_arithmetic(insn),
            _ => false,
        }
    }

    /// Checks if a jump, SETcc or CMOVcc can merge with the `cmp` before it.
    ///
    /// For example, both of these merge:
    ///
    /// ```text
    /// cmp eax, ebx
    /// jl  0x11000
    /// ```
    ///
    /// and...
    ///
    /// ```text
    /// cmp eax, ebx
    /// mov ecx, edx    ; doesn't touch flags, so it can sit in between
    /// jl  0x11000
    /// ```
    ///
    /// # Arguments
    ///
    /// - `compare`: the instruction that sets the flags, like `cmp`.
    /// - `next`: the instruction after it that reads the flags, like `jz`.
    ///
    /// # Returns
    ///
    /// - [`MergedInstructionKind::Jump`] for a jump to a fixed address.
    ///   The caller fills in which flags it needs.
    /// - [`MergedInstructionKind::Select`] for a SETcc or CMOVcc.
    /// - [`None`] if it can't merge.
    pub(in crate::codegen) fn can_merge(
        compare: &Insn,
        next: &Insn,
    ) -> Option<MergedInstructionKind> {
        let jump = try_as_jump(next.mnemonic())?;

        // Conditions C++ can't check directly, like `jo`, don't merge.
        Self::condition(compare.mnemonic(), jump)?;

        if next.instruction().meta.category == InstructionCategory::COND_BR {
            return is_direct_jump(next).then_some(MergedInstructionKind::Jump(CpuFlag::empty()));
        }

        accepts_select(next).then_some(MergedInstructionKind::Select)
    }

    /// What `reader` (a jump, SETcc or CMOVcc) checks after `compare`.
    ///
    /// # Returns
    ///
    /// [`None`] for conditions C++ can't check directly, like overflow.
    pub(in crate::codegen) fn condition(compare: Mnemonic, reader: Mnemonic) -> Option<Condition> {
        let jump = try_as_jump(reader)?;
        match compare {
            Mnemonic::COMISS | Mnemonic::UCOMISS | Mnemonic::FCOMIP | Mnemonic::FUCOMIP => {
                float_condition(jump)
            }
            Mnemonic::SAHF => sahf_condition(jump),
            Mnemonic::CMP => compare_condition(jump),
            // `sub eax, 1; jz` reads best as `eax == 0`. Style choice 😎
            Mnemonic::SUB => result_condition(jump).or_else(|| compare_condition(jump)), // Sewer note: sub and cmp are basically same in x86, cmp just discards result
            Mnemonic::TEST | Mnemonic::AND | Mnemonic::OR | Mnemonic::XOR => logic_condition(jump),
            Mnemonic::ADD => add_condition(jump),
            // INC, DEC, NEG and the shifts.
            _ => result_condition(jump),
        }
    }
}

/// Whether we can write this math instruction, like `sub` or `dec`.
///
/// ```text
/// sub  eax, 1           ; yes
/// dec  dword [esi]      ; yes
/// shl  eax, 4           ; yes
/// sub  esp, 8           ; no: stack passes track esp
/// shl  eax, 0           ; no: a 0 shift leaves the flags alone
/// shl  eax, cl          ; no: count might be 0
/// lock dec dword [esi]  ; no: we don't write atomic updates
/// ```
fn accepts_arithmetic(insn: &Insn) -> bool {
    let decoded = insn.instruction();

    // lock dec [esi]
    if decoded.attributes.contains(InstructionAttributes::HAS_LOCK) {
        return false;
    }

    let operands = insn.operands();
    let destination = &operands[0];
    let bits = destination.size;

    // esp, sp or spl
    let is_stack_pointer = register(destination)
        .is_some_and(|reg| reg.largest_enclosing(MachineMode::LEGACY_32) == Register::ESP);

    // eax, [esi], etc.
    let is_destination_ok = matches!(bits, 8 | 16 | 32)
        && immediate(destination).is_none()
        && !is_stack_pointer
        && is_direct_operand(destination, bits);
    if !is_destination_ok {
        return false;
    }

    let operand_count = decoded.operand_count_visible;
    match insn.mnemonic() {
        // dec eax
        Mnemonic::INC | Mnemonic::DEC | Mnemonic::NEG => operand_count == 1,

        // shl eax, 4
        Mnemonic::SHL | Mnemonic::SHR | Mnemonic::SAR => {
            let shifts = immediate(&operands[1]).is_some_and(|count| count.value & 31 != 0);
            operand_count == 2 && shifts
        }

        // sub eax, ebx
        _ => operand_count == 2 && is_direct_operand(&operands[1], bits),
    }
}

/// Whether we can write this `cmp` or `test`.
///
/// ```text
/// cmp  eax, ebx         ; yes
/// cmp  al, 5            ; yes
/// test [esi], eax       ; yes
/// cmp  eax, fs:[0x18]   ; no: FS memory
/// ```
fn accepts_integer(insn: &Insn) -> bool {
    if insn.instruction().operand_count_visible != 2 {
        return false;
    }

    let operands = insn.operands();
    let (left, right) = (&operands[0], &operands[1]);

    // 8, 16 or 32 bits.
    let bits = left.size;
    if !matches!(bits, 8 | 16 | 32) {
        return false;
    }

    is_direct_operand(left, bits) && is_direct_operand(right, bits)
}

/// Whether we can write this SETcc or CMOVcc.
///
/// ```text
/// setz  al              ; yes
/// setz  [esi]           ; yes
/// cmovz eax, ecx        ; yes
/// cmovz eax, [esi]      ; yes
/// cmovz eax, fs:[0x18]  ; no: FS memory
/// ```
fn accepts_select(insn: &Insn) -> bool {
    let operands = insn.operands();

    // eax, cl, etc.
    let is_register = |operand: &DecodedOperand| {
        register(operand).is_some_and(|reg| {
            matches!(
                reg.class(),
                RegisterClass::GPR8 | RegisterClass::GPR16 | RegisterClass::GPR32
            )
        })
    };

    // [esi], etc.
    let is_memory = |operand: &DecodedOperand| {
        memory(operand).is_some_and(|mem| matches!(mem.segment, Register::DS | Register::SS))
    };

    // setz al
    let is_setcc = insn.instruction().meta.category == InstructionCategory::SETCC;
    if is_setcc {
        let target = &operands[0];
        return is_register(target) || is_memory(target);
    }

    // cmovz eax, ecx
    let (target, source) = (&operands[0], &operands[1]);
    is_register(target) && (is_register(source) || is_memory(source))
}

/// Whether we can write this `comiss` or `ucomiss`.
///
/// ```text
/// comiss xmm0, xmm1         ; yes
/// comiss xmm0, [esi]        ; yes
/// comiss xmm0, fs:[0x18]    ; no: FS memory
/// ```
fn accepts_sse(insn: &Insn) -> bool {
    if usize::from(insn.instruction().operand_count) < 2 {
        return false;
    }

    let operands = insn.operands();
    let (left, right) = (&operands[0], &operands[1]);

    // xmm0 to xmm7
    let is_xmm = |operand: &DecodedOperand| register(operand).is_some_and(xmm_low);

    // [esi], etc.: a 32-bit float, not in FS or GS.
    let is_float_in_memory = memory(right).is_some_and(|mem| {
        let is_plain = matches!(mem.segment, Register::DS | Register::SS);
        right.size == 32 && is_plain
    });

    is_xmm(left) && (is_xmm(right) || is_float_in_memory)
}

/// Whether we can write this `fcomip` or `fucomip`.
///
/// ```text
/// fcomip st0, st1   ; yes
/// fcomip st1        ; yes: same, with st0 left out
/// ```
fn accepts_x87(insn: &Insn) -> bool {
    let operands = insn.operands();

    // The value compared with st0: the last operand.
    let source = match usize::from(insn.instruction().operand_count) {
        0 => return false,
        1 => &operands[0], // fcomip st1
        _ => &operands[1], // fcomip st0, st1
    };

    // st0 to st7
    let is_stack_register =
        |reg: Register| (Register::ST0 as u16..=Register::ST7 as u16).contains(&(reg as u16));
    register(source).is_some_and(is_stack_register)
}

/// The condition after ADD, which sets the CF, OF, SF, ZF and PF flags.
fn add_condition(jump: Mnemonic) -> Option<Condition> {
    Some(match jump {
        Mnemonic::JB => Condition::Carry(true),
        Mnemonic::JNB => Condition::Carry(false),
        _ => return result_condition(jump),
    })
}

/// The condition after CMP or SUB, which compare `lhs` with `rhs`.
fn compare_condition(jump: Mnemonic) -> Option<Condition> {
    Some(match jump {
        Mnemonic::JZ => Condition::Compare("==", false),
        Mnemonic::JNZ => Condition::Compare("!=", false),
        Mnemonic::JB => Condition::Compare("<", false),
        Mnemonic::JNB => Condition::Compare(">=", false),
        Mnemonic::JBE => Condition::Compare("<=", false),
        Mnemonic::JNBE => Condition::Compare(">", false),
        Mnemonic::JL => Condition::Compare("<", true),
        Mnemonic::JNL => Condition::Compare(">=", true),
        Mnemonic::JLE => Condition::Compare("<=", true),
        Mnemonic::JNLE => Condition::Compare(">", true),
        _ => return result_condition(jump),
    })
}

/// Whether `mnemonic` is math or logic that writes a result and flags.
fn is_arithmetic(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::ADD
            | Mnemonic::SUB
            | Mnemonic::AND
            | Mnemonic::OR
            | Mnemonic::XOR
            | Mnemonic::INC
            | Mnemonic::DEC
            | Mnemonic::NEG
            | Mnemonic::SHL
            | Mnemonic::SHR
            | Mnemonic::SAR
    )
}

/// Whether `jump` goes to a fixed address, like `jz 0x1234`.
fn is_direct_jump(jump: &Insn) -> bool {
    let decoded = jump.instruction();

    // Not a 16-bit jump.
    let is_32_bit = decoded.operand_width == 32;

    // Stays in the same code segment.
    let is_near = matches!(
        decoded.meta.branch_type,
        BranchType::SHORT | BranchType::NEAR
    );

    // Target is an imm32
    let has_fixed_target = immediate(&jump.operands()[0]).is_some_and(|imm| imm.is_relative);

    is_32_bit && is_near && has_fixed_target
}

/// The condition after TEST, AND, OR or XOR, which clear CF and OF.
///
/// Can be used in variety of ways:
/// - `jbe`, `jl` compare with zero
/// - `jb`, `jnb` test CF.
///
/// etc.
fn logic_condition(jump: Mnemonic) -> Option<Condition> {
    Some(match jump {
        Mnemonic::JBE => Condition::CompareToZero("<=", false),
        Mnemonic::JNBE => Condition::CompareToZero(">", false),
        Mnemonic::JL => Condition::CompareToZero("<", true),
        Mnemonic::JNL => Condition::CompareToZero(">=", true),
        Mnemonic::JLE => Condition::CompareToZero("<=", true),
        Mnemonic::JNLE => Condition::CompareToZero(">", true),
        _ => return result_condition(jump),
    })
}

/// The condition after SAHF, which copies SF, ZF, AF, PF and CF from `ah`.
fn sahf_condition(jump: Mnemonic) -> Option<Condition> {
    let (flags, set) = match jump {
        Mnemonic::JS => (CpuFlag::SF, true),
        Mnemonic::JNS => (CpuFlag::SF, false),
        _ => return float_condition(jump),
    };
    Some(Condition::Flags(flags, set))
}

/// The jump that tests the same condition as a SETcc or CMOVcc.
fn try_as_jump(reader: Mnemonic) -> Option<Mnemonic> {
    Some(match reader {
        Mnemonic::SETZ | Mnemonic::CMOVZ => Mnemonic::JZ,
        Mnemonic::SETNZ | Mnemonic::CMOVNZ => Mnemonic::JNZ,
        Mnemonic::SETB | Mnemonic::CMOVB => Mnemonic::JB,
        Mnemonic::SETNB | Mnemonic::CMOVNB => Mnemonic::JNB,
        Mnemonic::SETBE | Mnemonic::CMOVBE => Mnemonic::JBE,
        Mnemonic::SETNBE | Mnemonic::CMOVNBE => Mnemonic::JNBE,
        Mnemonic::SETL | Mnemonic::CMOVL => Mnemonic::JL,
        Mnemonic::SETNL | Mnemonic::CMOVNL => Mnemonic::JNL,
        Mnemonic::SETLE | Mnemonic::CMOVLE => Mnemonic::JLE,
        Mnemonic::SETNLE | Mnemonic::CMOVNLE => Mnemonic::JNLE,
        Mnemonic::SETS | Mnemonic::CMOVS => Mnemonic::JS,
        Mnemonic::SETNS | Mnemonic::CMOVNS => Mnemonic::JNS,
        Mnemonic::SETP | Mnemonic::CMOVP => Mnemonic::JP,
        Mnemonic::SETNP | Mnemonic::CMOVNP => Mnemonic::JNP,
        Mnemonic::JZ
        | Mnemonic::JNZ
        | Mnemonic::JB
        | Mnemonic::JNB
        | Mnemonic::JBE
        | Mnemonic::JNBE
        | Mnemonic::JL
        | Mnemonic::JNL
        | Mnemonic::JLE
        | Mnemonic::JNLE
        | Mnemonic::JS
        | Mnemonic::JNS
        | Mnemonic::JP
        | Mnemonic::JNP => reader,
        _ => return None,
    })
}

/// The condition after a float compare, which sets only CF, ZF and PF.
fn float_condition(jump: Mnemonic) -> Option<Condition> {
    let (flags, set) = match jump {
        Mnemonic::JB => (CpuFlag::CF, true),
        Mnemonic::JNB => (CpuFlag::CF, false),
        Mnemonic::JZ => (CpuFlag::ZF, true),
        Mnemonic::JNZ => (CpuFlag::ZF, false),
        Mnemonic::JBE => (CpuFlag::CF | CpuFlag::ZF, true),
        Mnemonic::JNBE => (CpuFlag::CF | CpuFlag::ZF, false),
        Mnemonic::JP => (CpuFlag::PF, true),
        Mnemonic::JNP => (CpuFlag::PF, false),
        _ => return None,
    };
    Some(Condition::Flags(flags, set))
}

/// Whether we can read `operand` as a plain value of `bits` size.
///
/// ```text
/// eax         ; yes
/// [esi]       ; yes
/// 5           ; yes
/// xmm0        ; no: not a general register
/// fs:[0x18]   ; no: FS memory needs special handling
/// ```
fn is_direct_operand(operand: &DecodedOperand, bits: u16) -> bool {
    // 5
    if immediate(operand).is_some() {
        return true;
    }

    let right_size = operand.size == bits;

    // eax, cl, etc.
    if let Some(reg) = register(operand) {
        let is_general = matches!(
            reg.class(),
            RegisterClass::GPR8 | RegisterClass::GPR16 | RegisterClass::GPR32
        );
        return right_size && is_general;
    }

    // [esi], etc.
    if let Some(mem) = memory(operand) {
        let is_plain = matches!(mem.segment, Register::DS | Register::SS);
        return right_size && is_plain;
    }

    false
}

/// The conditions that only look at the result: ZF, SF and PF.
fn result_condition(jump: Mnemonic) -> Option<Condition> {
    Some(match jump {
        Mnemonic::JZ => Condition::CompareToZero("==", false),
        Mnemonic::JNZ => Condition::CompareToZero("!=", false),
        Mnemonic::JS => Condition::Sign(true),
        Mnemonic::JNS => Condition::Sign(false),
        Mnemonic::JP => Condition::Parity(true),
        Mnemonic::JNP => Condition::Parity(false),
        _ => return None,
    })
}

/// Whether `reg` is XMM0 to XMM7, the registers the context holds.
fn xmm_low(reg: Register) -> bool {
    (Register::XMM0 as u16..=Register::XMM7 as u16).contains(&(reg as u16))
}
