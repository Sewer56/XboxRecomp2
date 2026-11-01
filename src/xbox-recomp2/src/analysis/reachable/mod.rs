//! Follow known functions and find more reachable code.
//!
//! - Visit known functions and follow their calls and jumps.
//! - Scan immediate pointers and follow configured switch cases.
//! - Revisit functions when new entries, configured edges, or pointers
//!   change the results.
//!
//! Stop when no affected functions remain. Whole-section fallback scans run
//! separately in the parent analysis module.

use super::graph::{DiscoveryGraph, EdgeKind, FunctionNode, LocalEdge};
use super::pointer_tables::{PointerScan, Resolution};
use super::walk::FunctionWalker;
use crate::config::{FunctionAddress, RecompilerConfig};
use crate::decode::Decoder;
use crate::executable::Executable;
use work_queue::WorkQueue;
use xbox_recomp2_executable::GuestAddress32;

mod work_queue;

/// Expands the graph until new entries, case blocks, and references stop appearing.
pub(super) fn expand(
    executable: &Executable,
    config: &RecompilerConfig,
    decoder: &Decoder,
    graph: &mut DiscoveryGraph,
) {
    let mut walker = FunctionWalker::new(executable, config, decoder);
    let mut pointers = PointerScan::new(executable, config, decoder);
    let mut pending = WorkQueue::new(graph);
    while let Some((entry, force_pointer_scan)) = pending.pop() {
        // Okay, so at this point, we have the:
        // - `entry`: The function entry being processed.
        // - `force_pointer_scan`: Scan even if the walk leaves the body unchanged.
        // - `pending`: The work queue of functions needing walks.

        // On first call, the `pending` work queue contains these symbols:
        // - Imported functions. (kernel thunks, import table, etc.)
        // - Other symbols in executable code.
        // - Entry point.
        // And we're vibing through these.

        // New entries or switch cases can put a function back in the queue.
        //
        // If we walked it before, forget the ranges and jump targets we tracked
        // last time, because the walk below rebuilds its body.
        pending.untrack_dependencies(&graph.functions[&entry]);
        let update = walker
            .walk_function(graph, entry)
            .expect("queued function exists");

        // Track what we reached this time, then queue the new functions we found.
        //
        // A new entry can mean this function needs another walk too, so track
        // its ranges and jump targets before notifying the queue.
        pending.track_dependencies(&graph.functions[&entry]);
        pending.learn_entries(update.new_functions);

        // A call can reveal a new entry in the body we just walked:
        //
        // ```text
        //   0x1000: ...            ; ecx has countdown
        //   0x1010: test ecx, ecx  ; not known entry
        //   0x1012: jz 0x101A      ; return when ecx is zero
        //   0x1014: dec ecx
        //   0x1015: call 0x1010    ; reveals the recursive entry
        //   0x101A: ret
        // ```
        //
        // i.e. 'function-in-function'.
        //
        // We'll need to walk again in such a case.
        // Because calls can enter 0x1010 without running code before it at
        // 0x1000.
        if pending.is_invalidated(entry) {
            continue;
        }

        // Skip unchanged walks unless new entries require another scan.
        if !update.changed && !force_pointer_scan {
            continue;
        }

        // Scan pointer operands without inferring register values.
        let function = &graph.functions[&entry];
        let references = scan_pointers(&mut pointers, function);

        apply_edges(config, graph, entry, &mut pending);
        apply_pointers(&walker, graph, entry, references, &mut pending);
    }
}

/// Adds configured switch targets to the next walk.
fn apply_edges(
    config: &RecompilerConfig,
    graph: &mut DiscoveryGraph,
    entry: GuestAddress32,
    pending: &mut WorkQueue,
) {
    let function = graph
        .functions
        .get_mut(&entry)
        .expect("queued function exists");

    // Iterate through all switch targets.
    for &source in function.instructions.keys() {
        if let Some(table) = config
            .switch_tables
            .get(FunctionAddress::from(source.get()))
        {
            // Go through all the labels.
            for &target in &table.labels {
                if function.learned.edges.insert(LocalEdge {
                    source,
                    target: GuestAddress32::from(target),
                    kind: EdgeKind::SwitchCase,
                }) {
                    // The last walk did not have this edge to follow.
                    pending.rewalk(entry);
                }
            }
        }
    }
}

/// Adds functions found through pointers and rechecks the walks they affect.
fn apply_pointers(
    walker: &FunctionWalker<'_>,
    graph: &mut DiscoveryGraph,
    entry: GuestAddress32,
    pointers: Resolution,
    pending: &mut WorkQueue,
) {
    for reference in pointers.references {
        if !walker.is_executable_target(reference.target) {
            continue;
        }

        // Check now.
        // And create func entry with add_reference.
        let is_new = !graph.functions.contains_key(&reference.target);
        if graph.add_reference(entry, reference) {
            pending.rewalk(entry);
        }

        // We may have walked through this address before knowing it was a
        // function entry. Need to recheck.
        if is_new {
            pending.learn_entries([reference.target]);
        }
    }
}

/// Finds pointer references in the function's current instructions.
fn scan_pointers(scan: &mut PointerScan<'_>, function: &FunctionNode) -> Resolution {
    let mut result = Resolution::default();
    for instruction in function.instructions.values() {
        scan.instruction(instruction, &mut result);
    }

    // One reference can turn up more than once during the scan.
    result.references.sort_unstable();
    result.references.dedup();

    result
}
