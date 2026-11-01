//! Recover function references from a linear scan of mapped code sections.

use super::code;
use crate::analysis::graph::{
    BruteForceKind, DiscoveryGraph, Flow, FunctionDiscoverySource, Instruction,
};
use crate::analysis::pointer_tables::{PointerScan, Resolution};
use crate::decode::{Decoder, Mnemonic};
use crate::executable::Executable;
use xbox_recomp2_executable::GuestAddress32;

/// Appends call and pointer targets.
///
/// ## Direct calls
///
/// ```text
/// 0x1000: E8 8C 00 00 00  call 0x1091 ; 0x1091 -> candidate
/// 0x1091: C3              ret
/// ```
///
/// ## Callbacks
///
/// ```text
/// 0x1000: 68 80 10 00 00  push 0x1080     ; callback candidate
///      or B8 80 10 00 00  mov eax, 0x1080 ; callback candidate
/// 0x1080: 55              push ebp        ; recognised entry instruction
/// ```
///
/// ## Pointer tables
///
/// ```text
/// 0x1000: 68 00 20 00 00        push 0x2000
///      or FF 14 85 00 20 00 00  call [eax*4 + 0x2000]
/// 0x2000: 91 10 00 00           dd 0x1091 ; candidate
/// ```
///
/// All targets must look callable. Only `call`, `push`, and `mov` contribute.
pub(super) fn collect(
    executable: &Executable,
    decoder: &Decoder,
    scan: &mut PointerScan<'_>,
    graph: &mut DiscoveryGraph,
) {
    let mut pointers = Resolution::default();
    code::for_each_decoded_instruction(executable, decoder, scan, |address, insn, _, scan| {
        // Skip instructions this reference scan cannot use.
        if matches!(
            insn.mnemonic(),
            Mnemonic::CALL | Mnemonic::PUSH | Mnemonic::MOV
        ) {
            let instruction = Instruction::from_decoded(address, insn);

            // Allow direct call.
            if instruction.flow == Flow::Call
                && let Some(target) = instruction.direct_target()
                && scan.is_callable(target)
            {
                add_to_graph(graph, target, BruteForceKind::DirectCall { site: address });
            }

            // Else reuse the pointer scan verdict from the brute force.
            scan.instruction(&instruction, &mut pointers);
        }
    });

    for reference in pointers.references {
        let kind = BruteForceKind::PossiblePointer {
            site: reference.site,
        };
        add_to_graph(graph, reference.target, kind);
    }
}

fn add_to_graph(graph: &mut DiscoveryGraph, target: GuestAddress32, kind: BruteForceKind) {
    graph.add_function(target, FunctionDiscoverySource::BruteForce { kind }, None);
}
