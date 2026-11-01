use super::prelude::{Mnemonic, Register, memory};
use crate::decode::Insn;

/// gs:, unrouted fs: and 16-bit addresses need more than `base + address`.
pub(in crate::codegen) fn is_supported(insn: &Insn) -> bool {
    if matches!(
        insn.mnemonic(),
        Mnemonic::LEA | Mnemonic::NOP | Mnemonic::PREFETCHT0 | Mnemonic::PREFETCHNTA
    ) {
        return true;
    }

    let instruction = insn.instruction();
    let count = usize::from(instruction.operand_count);
    let routes_fs = matches!(
        insn.mnemonic(),
        Mnemonic::MOV | Mnemonic::MOVZX | Mnemonic::MOVSX | Mnemonic::PUSH | Mnemonic::POP
    );
    let handles_a16 = matches!(
        insn.mnemonic(),
        Mnemonic::STOSB | Mnemonic::STOSW | Mnemonic::STOSD
    );

    insn.operands()[..count]
        .iter()
        .filter_map(memory)
        .all(|mem| {
            let segment = match mem.segment {
                Register::FS => {
                    routes_fs && mem.base == Register::NONE && mem.index == Register::NONE
                }
                Register::GS => false,
                _ => true,
            };
            let address =
                instruction.address_width == 32 || handles_a16 || mem.base == Register::ESP;
            segment && address
        })
}
