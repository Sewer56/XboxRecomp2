//! Reports how functions were found and which jumps or calls lack targets.

use super::graph::{DiscoveryGraph, FunctionDiscoverySource};
use std::collections::BTreeSet;
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

/// Writes discovery stats, unknown jump/call targets and walk stops to `diag`.
pub(super) fn report(
    graph: &DiscoveryGraph,
    from_roots: &BTreeSet<GuestAddress32>,
    diag: &mut dyn Write,
) {
    let mut roots = 0;
    let mut descendants = 0;
    let mut unresolved = 0;
    for (&entry, function) in &graph.functions {
        if !from_roots.contains(&entry) {
            if function
                .discovery_sources
                .iter()
                .any(|source| matches!(source, FunctionDiscoverySource::BruteForce { .. }))
            {
                roots += 1;
                let _ = writeln!(
                    diag,
                    "Fallback function 0x{:X}: not found from known entry points",
                    entry.get()
                );
            } else {
                descendants += 1;
            }
        }
        for transfer in &function.unresolved {
            if function
                .learned
                .edges
                .iter()
                .any(|edge| edge.source == transfer.site)
                || function
                    .learned
                    .references
                    .iter()
                    .any(|reference| reference.site == transfer.site)
            {
                continue;
            }
            unresolved += 1;
            let _ = writeln!(
                diag,
                "Unresolved {:?} at 0x{:X} in function 0x{:X}",
                transfer.kind,
                transfer.site.get(),
                entry.get()
            );
        }
        for diagnostic in &function.diagnostics {
            let _ = writeln!(
                diag,
                "Function 0x{:X}: stopped at 0x{:X} ({:?})",
                entry.get(),
                diagnostic.address.get(),
                diagnostic.reason
            );
        }
    }
    let _ = writeln!(
        diag,
        "Discovery: {} functions from known entry points, {roots} fallback functions, {descendants} functions reached from fallback, {unresolved} unresolved transfers",
        from_roots.len()
    );
}
