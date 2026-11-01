//! Keep decoded instruction details for analysis after the decoder moves on.

use crate::decode::{
    BranchType, DecodedOperandKind, Insn, InstructionAttributes, InstructionCategory, Mnemonic,
    OperandAction, Register,
};
use xbox_recomp2_executable::GuestAddress32;

/// An instruction with its operands and effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::analysis) struct Instruction {
    /// Guest address where the instruction starts.
    pub address: GuestAddress32,

    /// Number of encoded bytes. An x86 instruction is at most 15 bytes long.
    pub length: u8,

    /// Operation name reported by the decoder, such as `MOV` or `JMP`.
    pub mnemonic: Mnemonic,

    /// Width of the operation's values in bits, including a 16-bit override.
    pub operand_width: u8,

    /// Operands shown by the decoder. Hidden operands are not included here.
    pub operands: Box<[Operand]>,

    /// How execution continues, independent of the operand format.
    pub flow: Flow,

    /// Any/unknown register or flag may change here.
    ///
    /// Causes:
    /// - Special instruction (`wbinvd`).
    /// - Mid-assembly hook.
    ///
    /// UNSAFE TO OPTIMISE. BAKA.
    pub clobbers_all: bool,
}

/// How execution continues after an instruction.
///
/// This describes the path type, not whether the destination is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) enum Flow {
    /// `mov eax, ebx` continues at the next instruction.
    Next,

    /// `call eax` calls a function and can return afterward.
    Call,

    /// `jmp eax` does not continue at the next instruction.
    Jump,

    /// `jnz 0x1010` can branch or continue at the next instruction.
    Conditional,

    /// `ret` leaves the current function.
    Return,

    /// `int3` stops this path without a known next instruction.
    Stop,

    /// `hlt` or a far jump cannot be followed by the walker.
    Unsupported,
}

/// An operand and width of its value in bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) struct Operand {
    /// Width of the value in bits.
    ///
    /// Sewer Note:
    ///
    /// - On Xbox this would be fine as u8, but as this can be wrangled
    ///   to be a more general purpose recompiler; I use u16, because
    ///   we'll certainly need u16 here for AVX.
    /// - Or I could use an enum with common sizes- but eh; same mem struct size.
    pub width: u16,

    /// Where the value comes from: a register, an immediate, or memory.
    pub kind: OperandKind,
}

/// Type of operand used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) enum OperandKind {
    /// A register value, as in `mov eax, ebx`.
    Register(Register),

    /// An immediate value, either:
    ///
    /// - A branch offset, as in `jmp 0x1010` (`relative: true`)
    /// - A literal, as in `push 0x2000` (`relative: false`)
    Immediate { value: u64, relative: bool },

    /// A value in memory, addressed using registers or a fixed address.
    Memory(MemoryOperand),

    /// An operand form this code cannot represent, such as the far pointer in
    /// `call far 0x08:0x2000`.
    Unsupported,
}

/// The pieces used to calculate a memory address.
///
/// For `fs:[ebx+ecx*4-8]`, the address uses:
///
/// - Segment: `FS`
/// - Base register: `EBX`
/// - Index register: `ECX`, multiplied by 4
/// - Displacement: -8
///
/// [`Operand`] stores the value's width separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::analysis) struct MemoryOperand {
    /// Segment used by the access, including overrides such as `fs:`.
    pub segment: Register,

    /// Base register, or `Register::NONE` when there is no base.
    pub base: Register,

    /// Index register, or `Register::NONE` when there is no index.
    pub index: Register,

    /// Index multiplier, or zero when the index is not scaled.
    pub scale: u8,

    /// Signed offset added to the address, or zero when absent.
    pub displacement: i64,

    /// Address width in bits. It can differ from the value width.
    pub address_width: u8,
}

impl Instruction {
    /// Copies analysis details from a decoded instruction.
    ///
    /// # Arguments
    ///
    /// - `address`: Guest address where `insn` starts.
    /// - `insn`: Decoded instruction to copy.
    ///
    /// # Remarks
    ///
    /// The decoder reuses its internal buffer, so clone/copy before decoding
    /// the next one.
    pub fn from_decoded(address: GuestAddress32, insn: &Insn) -> Self {
        // Get the decoded instruction.
        let decoded = insn.instruction();

        // Translate visible operands into our operand types.
        let operands = insn.operands()[..usize::from(decoded.operand_count_visible)]
            .iter()
            .map(|operand| Operand {
                width: operand.size,
                kind: match &operand.kind {
                    DecodedOperandKind::Reg(reg) => OperandKind::Register(*reg),
                    DecodedOperandKind::Imm(imm) => OperandKind::Immediate {
                        value: imm.value,
                        relative: imm.is_relative,
                    },
                    DecodedOperandKind::Mem(mem) => OperandKind::Memory(MemoryOperand {
                        segment: mem.segment,
                        base: mem.base,
                        index: mem.index,
                        scale: mem.scale,
                        displacement: mem.disp.displacement,
                        address_width: decoded.address_width,
                    }),
                    _ => OperandKind::Unsupported,
                },
            })
            .collect();

        // Special instructions and segment register writes can make
        // optimisations unsafe due to 'le clobber'.
        //
        // Check all operands, since hidden ones can be dangerous too.
        let mut clobbers_all = decoded
            .attributes
            .intersects(InstructionAttributes::IS_PRIVILEGED | InstructionAttributes::CPU_STATE_CW);
        for operand in &insn.operands()[..usize::from(decoded.operand_count)] {
            if !operand
                .action
                .intersects(OperandAction::WRITE | OperandAction::CONDWRITE)
            {
                continue;
            }
            if let DecodedOperandKind::Reg(reg) = operand.kind {
                clobbers_all |= matches!(
                    reg,
                    Register::CS
                        | Register::DS
                        | Register::ES
                        | Register::FS
                        | Register::GS
                        | Register::SS
                );
            }
        }

        // Determine control flow.
        let flow = if decoded.meta.branch_type == BranchType::FAR
            || matches!(
                insn.mnemonic(),
                Mnemonic::UD0
                    | Mnemonic::UD1
                    | Mnemonic::UD2
                    | Mnemonic::HLT
                    | Mnemonic::XBEGIN
                    | Mnemonic::XABORT
                    | Mnemonic::XEND
            ) {
            Flow::Unsupported
        } else {
            match decoded.meta.category {
                InstructionCategory::CALL => Flow::Call,
                InstructionCategory::COND_BR => Flow::Conditional,
                InstructionCategory::UNCOND_BR => Flow::Jump,
                InstructionCategory::RET => Flow::Return,
                InstructionCategory::INTERRUPT if insn.mnemonic() == Mnemonic::INT3 => Flow::Stop,
                // These supported system operations return to the next guest instruction.
                InstructionCategory::SYSTEM
                    if matches!(
                        insn.mnemonic(),
                        Mnemonic::WBINVD | Mnemonic::RDTSC | Mnemonic::SGDT | Mnemonic::SLDT
                    ) =>
                {
                    Flow::Next
                }
                InstructionCategory::INTERRUPT
                | InstructionCategory::SYSCALL
                | InstructionCategory::SYSRET
                | InstructionCategory::SYSTEM => Flow::Unsupported,
                _ => Flow::Next,
            }
        };

        Self {
            address,
            length: decoded.length,
            mnemonic: insn.mnemonic(),
            operand_width: decoded.operand_width,
            operands,
            flow,
            clobbers_all,
        }
    }

    /// Returns the address just past this instruction's last byte.
    ///
    /// # Returns
    ///
    /// A [`u64`] with the address.
    pub fn end(&self) -> u64 {
        u64::from(self.address.get()) + u64::from(self.length)
    }

    /// Returns the branch target when the first operand is a relative offset.
    ///
    /// # Returns
    ///
    /// - [`Some`] for `jmp 0x1010` or `jnz 0x1010`.
    /// - [`None`] for `jmp eax` or `jmp [eax]`.
    ///
    /// # Remarks
    ///
    /// With a 16-bit operand size, the target is cut to its low 16 bits.
    /// (It's just what the x86 CPU does; prolly for legacy reasons.)
    pub fn direct_target(&self) -> Option<GuestAddress32> {
        let OperandKind::Immediate {
            value,
            relative: true,
        } = self.operands.first()?.kind
        else {
            return None;
        };

        let target = (self.end() as u32).wrapping_add(value as u32);
        Some(GuestAddress32::from(if self.operand_width == 16 {
            target & 0xffff
        } else {
            target
        }))
    }
}
