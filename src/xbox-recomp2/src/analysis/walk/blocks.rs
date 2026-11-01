//! Group reachable instructions into straight-line blocks.

use crate::analysis::graph::{BasicBlock, EdgeKind, FunctionNode, LocalEdge};
use std::collections::{BTreeMap, BTreeSet};
use xbox_recomp2_executable::GuestAddress32;

/// Groups consecutive instructions until control flow splits or joins.
///
/// Start a new block at:
/// - A known entry, where another caller may enter with unknown state.
/// - A jump target or a target shared by paths.
pub(super) fn build(
    node: &FunctionNode,
    edges: &BTreeMap<GuestAddress32, Vec<LocalEdge>>,
    known_entries: &BTreeMap<GuestAddress32, FunctionNode>,
) -> Vec<BasicBlock> {
    // Example: two paths meet at 0x1005.
    //   0x1000: test eax, eax
    //   0x1002: jnz 0x1005
    //   0x1004: nop
    //   0x1005: ret

    // Here, only 0x1000 is a known entry. The jnz reaches 0x1004 or 0x1005,
    // and the nop also reaches 0x1005.

    // Get instruction offsets that are known function entries first.
    // i.e. Starts of each block.
    let mut isns_offsets = BTreeSet::from([node.entry]);
    isns_offsets.extend(
        node.instructions
            .keys()
            .filter(|address| known_entries.contains_key(address)),
    );

    // Now look through the local jumps (edges), and add those too.
    let mut seen_targets = BTreeSet::new();
    for edge_values in edges.values() {
        for edge in edge_values {
            // An edge that isn't a 'fallthrough' (branch not taken)
            // or second incoming edge starts a new block.
            let already_seen_target = !seen_targets.insert(edge.target);
            if edge.kind != EdgeKind::NextInstruction || already_seen_target {
                isns_offsets.insert(edge.target);
            }
        }
    }

    // So now we have:
    // - 0x1000: the function entry
    // - 0x1004: when jnz is not taken
    // - 0x1005: when jnz is taken

    // Now start turning these starts into blocks.
    let mut visited_addresses = BTreeSet::new();
    let mut blocks = Vec::with_capacity(isns_offsets.len().min(node.instructions.len()));
    for &block_start in node.instructions.keys() {
        // If we've been here, due to a dupe (somehow??) then yeet out.
        if visited_addresses.contains(&block_start) {
            continue;
        }

        // Instructions for this block; and current address.
        let mut instructions = Vec::new();
        let mut address = block_start;
        let successors = loop {
            // Mark the current as visited.
            visited_addresses.insert(address);
            instructions.push(address);

            // Get the successors/edges.
            let successors = edges.get(&address).map(Vec::as_slice).unwrap_or_default();

            // Are we done yet?
            // If next instruction in line is one we haven't visited, and
            // is at next address, we can continue.
            if let [edge] = successors
                // branch not taken
                && edge.kind == EdgeKind::NextInstruction
                // have we been here already?
                && !isns_offsets.contains(&edge.target)
                && !visited_addresses.contains(&edge.target)
                // is it in this function?
                && node.instructions.contains_key(&edge.target)
            {
                address = edge.target;
                continue;
            }

            // The block keeps the final instruction's outgoing edges.
            break successors.to_vec();
        };

        blocks.push(BasicBlock {
            start: block_start,
            instructions,
            successors,
        });
    }

    blocks
}
