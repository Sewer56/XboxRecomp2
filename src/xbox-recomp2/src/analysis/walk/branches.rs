//! Follow paths inside a function and save paths to other functions.

use crate::analysis::graph::{
    Diagnostic, EdgeKind, Flow, FunctionNode, FunctionReference, Instruction, LearnedTargets,
    LocalEdge, ReferenceKind, StopReason, TransferKind, UnresolvedTransfer,
};
use std::collections::BTreeMap;
use xbox_recomp2_executable::{GuestAddress32, GuestAddress64};

/// Returns local paths; saves direct calls and tail calls separately.
///
/// We log:
/// - `call 0x81234567` / `jmp 0x81234567`
/// - `jz 0x81234567` and other `jcc`
/// - `call eax` / `jmp eax` / `jmp [eax]`
pub(super) fn local_successors(
    node: &mut FunctionNode,
    instruction: &Instruction,
    known_entries: &BTreeMap<GuestAddress32, FunctionNode>,
    learned: &LearnedTargets,
) -> Vec<LocalEdge> {
    let site = instruction.address;
    let mut edges = Vec::new();
    match instruction.flow {
        Flow::Next | Flow::Call => {
            // Save the callee separately, then continue after the call.
            if instruction.flow == Flow::Call {
                if let Some(target) = instruction.direct_target() {
                    node.references.insert(FunctionReference {
                        site,
                        target,
                        kind: ReferenceKind::DirectCall,
                    });
                } else {
                    unresolved(node, site, TransferKind::Call);
                }
            }
            fallthrough(node, instruction, EdgeKind::NextInstruction, &mut edges);
        }
        Flow::Jump | Flow::Conditional => {
            // There's a few types of jumps here:
            // - Local jumps (same function)
            // - Tail calls (jmp to another function)
            // - Indirect jumps (through registers or memory)
            //
            // The indirect ones we leave for later. The direct ones we resolve
            // NOW.
            if let Some(target) = instruction.direct_target() {
                let kind = if instruction.flow == Flow::Jump {
                    EdgeKind::Jump
                } else {
                    EdgeKind::BranchTaken
                };
                target_edge(
                    node,
                    LocalEdge {
                        source: site,
                        target,
                        kind,
                    },
                    known_entries,
                    &mut edges,
                );
            } else {
                // Jump is indirect, e.g. `jmp eax` or `jmp [eax]`.
                let kind = if instruction.flow == Flow::Jump {
                    TransferKind::Jump
                } else {
                    TransferKind::Conditional
                };
                unresolved(node, site, kind);
            }

            // Some jumps are 'conditional', e.g. 'jnz' or 'je'.
            // Might not always fire; because they feel like being tsundere.
            if instruction.flow == Flow::Conditional {
                fallthrough(node, instruction, EdgeKind::BranchNotTaken, &mut edges);
            }
        }
        Flow::Unsupported => {
            unresolved(node, site, TransferKind::Unsupported);
            node.diagnostics.insert(Diagnostic {
                address: site,
                reason: StopReason::UnsupportedControlFlow,
            });
        }
        Flow::Return | Flow::Stop => {}
    }

    // Okay. Remember `function-discovery.md`?
    // This is that part, where we reuse the brute force results,
    // on the second walk.
    if matches!(instruction.flow, Flow::Jump | Flow::Conditional) {
        // Resolve targets from the brute force search.
        let resolved_edges_at_site = learned
            .edges
            .range(
                LocalEdge {
                    source: site,
                    target: GuestAddress32::from(0),
                    kind: EdgeKind::NextInstruction,
                }..,
            )
            .take_while(|edge| edge.source == site)
            .copied();
        for edge in resolved_edges_at_site {
            target_edge(node, edge, known_entries, &mut edges);
        }
    }

    // Copy references from learned.references to node.references when
    // this is called again. So we merge the brute force results in.
    node.references.extend(
        learned
            .references
            .range(
                FunctionReference {
                    site,
                    target: GuestAddress32::from(0),
                    kind: ReferenceKind::DirectCall,
                }..,
            )
            .take_while(|reference| reference.site == site),
    );
    edges.sort_unstable_by_key(|edge| (edge.target, edge.kind));
    edges.dedup();

    edges
}

/// Whether a jump to `target` leaves `node`'s body.
pub(super) fn outside_body(node: &FunctionNode, target: GuestAddress32) -> bool {
    let entry = node.entry.get();
    let target = target.get();
    match node.authoritative_size {
        // Somewhere outside the range.
        Some(size) => target <= entry || target >= entry.wrapping_add(size.0),
        // No size, so no idea where the body ends.
        // Only the entry (first instruction) is guaranteed ours.
        None => target != entry,
    }
}

/// Restores a tail call as a local edge when another path reaches its target.
///
/// For example, `0x1005` a known function entry:
///
/// ```asm
/// 0x1000: je  0x1004
/// 0x1002: jmp 0x1005
/// 0x1004: nop
/// 0x1005: ret
/// ```
///
/// The jump initially looks like a tail call to `0x1005` (i.e. jump to
/// another function).
///
/// However, later discovery teaches us, the existing function can branch
/// to 0x1004. So, that jump earlier was not a tail-call. We need to fix
/// that right meow!
pub(super) fn reclassify_tail_calls_as_local_edges(
    node: &mut FunctionNode,
    edges: &mut BTreeMap<GuestAddress32, Vec<LocalEdge>>,
    learned: &LearnedTargets,
) {
    let mut restored_any = false;

    // We reconsider tail calls only when an automatic walk reaches their targets.
    node.references.retain(|reference| {
        // Ignore non-tailcall references.
        if reference.kind != ReferenceKind::TailCall
            || node.authoritative_size.is_some()
            || !node.instructions.contains_key(&reference.target)
        {
            return true;
        }
        let instruction = &node.instructions[&reference.site];

        // Restore the kind of path that originally led to this target.
        let edge_kind =
            // It may be a branch.
            if instruction.direct_target() == Some(reference.target) {
                match instruction.flow {
                    Flow::Jump => EdgeKind::Jump,
                    Flow::Conditional => EdgeKind::BranchTaken,
                    _ => return true,
                }
        }
        // A switch case may lead here! Or any ptr jump really!
        else if learned.edges.contains(&LocalEdge {
            source: reference.site,
            target: reference.target,
            kind: EdgeKind::SwitchCase,
        }) {
            EdgeKind::SwitchCase
        } else {
            // Register jumps still use the function table.
            return true;
        };

        // Aight, we can convert to the local edge.
        // We return false to nuke original, and add our own.
        let successors = edges.get_mut(&reference.site).expect("reachable site");
        successors.push(LocalEdge {
            source: reference.site,
            target: reference.target,
            kind: edge_kind,
        });
        restored_any = true;
        false
    });

    if restored_any {
        // A restored edge may duplicate one already found before, FYI
        for successors in edges.values_mut() {
            successors.sort_unstable_by_key(|edge| (edge.target, edge.kind));
            successors.dedup();
        }
    }
}

/// Queues the address after an instruction for later traversal.
/// e.g. After a `call` instruction, when we return.
/// or after `jmp` that is not taken.
///
/// # Arguments
///
/// - `node`: Function that receives an overflow diagnostic if needed.
/// - `instruction`: Instruction whose end gives the next address.
/// - `kind`: Edge type, such as `NextInstruction` or `BranchNotTaken`.
/// - `edges`: Local paths to traverse.
///
/// If the next address cannot fit in a 32-bit guest address, notes
/// `AddressOverflow` at the instruction instead of queuing an edge.
fn fallthrough(
    node: &mut FunctionNode,
    instruction: &Instruction,
    kind: EdgeKind,
    edges: &mut Vec<LocalEdge>,
) {
    if let Ok(target) = GuestAddress32::try_from(GuestAddress64::from(instruction.end())) {
        edges.push(LocalEdge {
            source: instruction.address,
            target,
            kind,
        });
    } else {
        node.diagnostics.insert(Diagnostic {
            address: instruction.address,
            reason: StopReason::AddressOverflow,
        });
    }
}

/// Classifies an edge as a tail call or a local path.
///
/// - Tail call: add a reference to the target function.
/// - Local path: queue the edge for traversal.
///
/// # Arguments
///
/// - `node`: Function whose tail-call references may change.
/// - `edge`: Candidate path to classify.
/// - `known_entries`: Function entries that may be tail-call targets.
/// - `edges`: Local paths to traverse after classification.
fn target_edge(
    node: &mut FunctionNode,
    edge: LocalEdge,
    known_entries: &BTreeMap<GuestAddress32, FunctionNode>,
    edges: &mut Vec<LocalEdge>,
) {
    // Eligible edges to known entries outside this body are tail calls.
    // That's basically it 😉
    if edge.kind.can_tail_call()
        && known_entries.contains_key(&edge.target)
        && outside_body(node, edge.target)
    {
        node.references.insert(FunctionReference {
            site: edge.source,
            target: edge.target,
            kind: ReferenceKind::TailCall,
        });
    } else {
        edges.push(edge);
    }
}

fn unresolved(node: &mut FunctionNode, site: GuestAddress32, kind: TransferKind) {
    node.unresolved.insert(UnresolvedTransfer { site, kind });
}

#[cfg(test)]
mod tests {
    use crate::analysis::graph::test_helpers::{base, function};
    use crate::decode::Mnemonic;
    use rstest::rstest;

    #[rstest]
    #[case::ret(&[0xc3, 0x90], Mnemonic::RET)]
    #[case::int3(&[0xcc, 0x90], Mnemonic::INT3)]
    #[case::indirect_jump(&[0xff, 0xe0, 0x90], Mnemonic::JMP)]
    #[case::unsupported(&[0x0f, 0x0b, 0x90], Mnemonic::UD2)]
    #[case::far_jump(&[0xea, 0, 0x20, 0, 0, 8, 0, 0x90], Mnemonic::JMP)]
    #[case::transaction(&[0xc7, 0xf8, 0, 0, 0, 0, 0x90], Mnemonic::XBEGIN)]
    fn walk_stops_before_trailing_nop(#[case] code: &[u8], #[case] mnemonic: Mnemonic) {
        // The walk should stop before the nop after each instruction.
        // When a control transferring instruction is encountered.
        // It treats XBEGIN as unsupported, so the walk stops there too.

        // ```text
        // ret:
        // 0x1000: C3                    ret
        // 0x1001: 90                    nop
        // int3:
        // 0x1000: CC                    int3
        // 0x1001: 90                    nop
        // indirect_jump:
        // 0x1000: FF E0                 jmp eax
        // 0x1002: 90                    nop
        // unsupported:
        // 0x1000: 0F 0B                 ud2
        // 0x1002: 90                    nop
        // far_jump:
        // 0x1000: EA 00 20 00 00 08 00  jmp far 0x0008:0x2000
        // 0x1007: 90                    nop
        // transaction:
        // 0x1000: C7 F8 00 00 00 00     xbegin 0x1006
        // 0x1006: 90                    nop
        // ```
        let node = function(code);

        assert_eq!(node.instructions.len(), 1);
        assert_eq!(node.instructions[&base()].mnemonic, mnemonic);
    }
}
