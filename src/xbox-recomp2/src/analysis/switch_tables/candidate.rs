//! Recognises `jmp [index*4 + table]` and reads case targets from its table.
//!
//! See "Discover switch tables" in `doc/docs/function-discovery.md` for a
//! worked example.

use super::bounds_check::BoundsCheck;
use crate::config::Register;
use crate::decode::{
    DecodeBuffer, DecodeOutcome, Decoder, Mnemonic, Register as IndexRegister, memory,
};
use crate::executable::{Executable, SectionFlags};
use endian_writer::{EndianReader, LittleEndianReader};

/// A table jump found by the scan.
pub(super) struct Candidate {
    /// Address of the `jmp` itself.
    /// Same as `base` in the `[[switch]]` config entry.
    pub(super) jump: u64,

    /// Address of the table the `jmp` reads from. (table base)
    pub(super) address: u64,

    /// Register that picks the table slot.
    pub(super) register: Register,

    /// Nearby bounds check and the slot count it allows, if one was found.
    pub(super) bounds: Option<BoundsCheck>,
}

impl Candidate {
    /// Checks for a potential `jmp [index*4 + table]` at `offset`.
    pub(super) fn at(
        executable: &Executable,
        decoder: &Decoder,
        buffer: &mut DecodeBuffer,
        bytes: &[u8],
        offset: usize,
        section_base: u64,
    ) -> Option<Self> {
        let candidate = Self::decode(decoder, buffer, bytes, offset, section_base)?;
        mapped_bytes(executable, candidate.address)?;
        Some(candidate)
    }

    /// Like [`Self::at`], but doesn't check that the table has bytes in the file.
    fn decode(
        decoder: &Decoder,
        buffer: &mut DecodeBuffer,
        bytes: &[u8],
        offset: usize,
        section_base: u64,
    ) -> Option<Self> {
        let DecodeOutcome::Ok(insn) =
            buffer.decode_single_instruction(decoder, bytes.get(offset..)?)
        else {
            return None;
        };

        let operand = insn.operands().first()?;
        let mem = memory(operand)?;

        // Opcode FF encodes other instructions too. (owo)
        // Accept only a jmp loading a 32-bit target.
        if insn.mnemonic() != Mnemonic::JMP || operand.size != 32 {
            return None;
        }

        // Slots are 4 bytes each; and we need index register to pick.
        if mem.scale != 4 || mem.index == IndexRegister::NONE {
            return None;
        }

        let address = mem.disp.displacement as u64;

        Some(Self {
            jump: section_base.checked_add(offset as u64)?,
            address,
            register: config_register(mem.index)?,
            bounds: None,
        })
    }

    /// Reads targets until the table ends or a target isn't code.
    pub(super) fn labels(&self, executable: &Executable, starts: &[u64]) -> Vec<u32> {
        labels(executable, self.address, starts)
    }
}

/// Returns true if `address` is in a code section and stored in the file.
pub(super) fn is_code(executable: &Executable, address: u64) -> bool {
    executable
        .section_containing(address)
        .is_some_and(|section| {
            section.flags.contains(SectionFlags::CODE)
                && (address - section.base.get())
                    < section.file_bytes(&executable.data).len() as u64
        })
}

/// Returns the stored bytes from `address` onward.
pub(super) fn mapped_bytes(executable: &Executable, address: u64) -> Option<&[u8]> {
    executable.find(address)
}

/// Converts a decoded index register to the config's [`Register`].
fn config_register(index: IndexRegister) -> Option<Register> {
    Some(match index {
        IndexRegister::EAX => Register::Eax,
        IndexRegister::ECX => Register::Ecx,
        IndexRegister::EDX => Register::Edx,
        IndexRegister::EBX => Register::Ebx,
        IndexRegister::EBP => Register::Ebp,
        IndexRegister::ESI => Register::Esi,
        IndexRegister::EDI => Register::Edi,
        _ => return None,
    })
}

fn labels(executable: &Executable, address: u64, starts: &[u64]) -> Vec<u32> {
    let Some(bytes) = mapped_bytes(executable, address) else {
        return Vec::new();
    };

    read_labels(address, bytes, starts, |target| is_code(executable, target))
}

/// Reads targets from `bytes`, using `is_code` to check each one.
fn read_labels(
    address: u64,
    bytes: &[u8],
    starts: &[u64],
    is_code: impl Fn(u64) -> bool,
) -> Vec<u32> {
    let next = starts.partition_point(|&start| start <= address);
    let count = starts.get(next).map_or(bytes.len() / 4, |&next| {
        ((next - address) / 4).min((bytes.len() / 4) as u64) as usize
    });

    let mut reader = unsafe { LittleEndianReader::new(bytes.as_ptr()) };
    let mut labels = Vec::new();
    for _ in 0..count {
        let label = unsafe { reader.read_u32() };
        let address = u64::from(label);
        if !is_code(address) {
            break;
        }
        labels.push(label);
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── Instruction recognition ──────────────────────────────────────────────

    #[rstest]
    // jmp [ebx*4 + 0x11400]
    #[case::ebx(&[0xFF, 0x24, 0x9D, 0x00, 0x14, 0x01, 0x00], Some(Register::Ebx))]
    // jmp [esi*4 + 0x11400]
    #[case::esi(&[0xFF, 0x24, 0xB5, 0x00, 0x14, 0x01, 0x00], Some(Register::Esi))]
    // jmp [ebp + eax*4 + 0x11400]: resolution ignores ebp.
    #[case::base_register(&[0xFF, 0xA4, 0x85, 0x00, 0x14, 0x01, 0x00], Some(Register::Eax))]
    // jmp [eax*2 + 0x11400]: not four-byte slots.
    #[case::scale_two(&[0xFF, 0x24, 0x45, 0x00, 0x14, 0x01, 0x00], None)]
    // jmp [0x11400]: SIB index 100 means no index, not esp.
    #[case::no_index(&[0xFF, 0x24, 0xA5, 0x00, 0x14, 0x01, 0x00], None)]
    fn decode_should_recognise_supported_dispatches(
        #[case] bytes: &[u8],
        #[case] expected_register: Option<Register>,
    ) {
        // Arrange
        //
        // - Each case supplies exactly one instruction, shown above it.
        // - The slice starts at guest address 0x11008.
        // - The index register selects a 4-byte slot in table storage at 0x11400.
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // Act.
        let candidate = Candidate::decode(&decoder, &mut buffer, bytes, 0, 0x11008);

        // Assert
        // Accepted instructions decode both addresses and the index.
        assert_eq!(
            candidate.as_ref().map(|value| value.register),
            expected_register,
        );
        if let Some(candidate) = candidate {
            assert_eq!(candidate.jump, 0x11008);
            assert_eq!(candidate.address, 0x11400);
            assert!(candidate.bounds.is_none());
        }
    }

    // ── Fallback slot collection ─────────────────────────────────────────────

    /// These are table data, not instruction bytes.
    #[test]
    fn read_labels_should_ignore_partial_final_slot() {
        // Arrange
        //
        // ```text
        //   0x11400: 30 10 01 00   dd 0x11030
        //   0x11404: 34 10 01      only three bytes of dd 0x11034
        // ```
        let bytes = [
            0x30, 0x10, 0x01, 0x00, // dd 0x11030: complete code pointer
            0x34, 0x10, 0x01, // truncated dd 0x11034: missing its fourth byte
        ];

        // Act.
        let labels = read_labels(0x11400, &bytes, &[], |_| true);

        // Assert
        // Do not read or zero-fill the incomplete second slot.
        assert_eq!(labels, [0x11030]);
    }

    #[test]
    fn read_labels_should_keep_all_slots_when_count_exceeds_byte_range() {
        // Arrange
        // 300 complete targets, more than an 8-bit count can represent.
        //
        // ```text
        //   0x11400..=0x118AF: 300 copies of dd 0x11030
        // ```
        //
        // However, we should keep all 300 slots; it's still useful to know
        // down the road.
        let expected = vec![0x11030; 300];
        let bytes: Vec<_> = expected
            .iter()
            .flat_map(|value: &u32| value.to_le_bytes())
            .collect();

        // Act.
        let labels = read_labels(0x11400, &bytes, &[], |_| true);

        // Assert
        assert_eq!(labels, expected);
    }
}
