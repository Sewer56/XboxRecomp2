//! Builds new instructions for passes that rewrite guest code.
//!
//! - [`encode`] turns a mnemonic and operands into an [`Insn`].
//! - [`Operand`] is one operand to encode.

use super::{DecodeBuffer, DecodeOutcome, Decoder, Insn, Mnemonic, Register};
use zydis::{EncoderOperand, EncoderRequest, MAX_INSTRUCTION_LENGTH, MachineMode};

/// One operand for [`encode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    /// A register, like `eax`.
    Reg(Register),

    /// A 32-bit memory operand, `dword [base + disp]`.
    Mem {
        /// The base register.
        base: Register,

        /// The displacement added to it.
        disp: i32,
    },

    /// An immediate value.
    Imm(u32),
}

/// Encodes `mnemonic` with `operands`, then decodes it back.
///
/// # Examples
///
/// Operands go in Intel order, destination first.
///
/// ```text
/// // xor eax, 0x04
/// let mask = encode(
///     Mnemonic::XOR,
///     &[Operand::Reg(Register::EAX), Operand::Imm(4)],
/// );
///
/// // movss xmm0, dword ptr [esp+0x04]
/// let load = encode(
///     Mnemonic::MOVSS,
///     &[
///         Operand::Reg(Register::XMM0),
///         Operand::Mem { base: Register::ESP, disp: 4 },
///     ],
/// );
///
/// // movd xmm0, eax
/// let from_gpr = encode(
///     Mnemonic::MOVD,
///     &[Operand::Reg(Register::XMM0), Operand::Reg(Register::EAX)],
/// );
///
/// // xorps xmm0, xmm0
/// let clear = encode(Mnemonic::XORPS, &[Operand::Reg(Register::XMM0); 2]);
/// ```
///
/// # Panics
///
/// If Zydis can't encode the instruction.
pub(crate) fn encode(mnemonic: Mnemonic, operands: &[Operand]) -> Insn {
    let mut request = EncoderRequest::new(MachineMode::LEGACY_32, mnemonic);
    for operand in operands {
        let operand = match *operand {
            Operand::Reg(reg) => EncoderOperand::reg(reg),
            Operand::Mem { base, disp } => EncoderOperand::mem_base_disp(4, base, disp),
            Operand::Imm(value) => EncoderOperand::imm(u64::from(value)),
        };
        request = request.add_operand(operand);
    }
    let mut bytes = [0u8; MAX_INSTRUCTION_LENGTH];
    let length = request
        .encode_into(&mut bytes)
        .unwrap_or_else(|status| panic!("can't encode {mnemonic:?} {operands:?}: {status:?}"));

    // Decode it back, so we can interleave with regular instructions from xbe.
    let mut buffer = DecodeBuffer::new();
    match buffer.decode_single_instruction(&Decoder::new(), &bytes[..length]) {
        DecodeOutcome::Ok(insn) => insn.clone(),
        DecodeOutcome::Failed { status, .. } => {
            panic!("encoded {mnemonic:?} doesn't decode: {status:?}")
        }
    }
}
