//! Prepare discovered functions for code generation.
//!
//! The generator needs to know which functions to emit and which instructions
//! belong to each one. Finalisation turns discovery's findings into those inputs.
//!
//! Here's roughly what we do:
//!
//! - Apply manual sizes to functions.
//! - Figure out function sizes based on decodewd instructions and branches.
//! - Preserve function references for later analysis.

use super::graph::{DiscoveryGraph, Flow, FunctionDiscoverySource};
use crate::config::{FunctionAddress, RecompilerConfig};
pub use function::Function;
pub use function_body::{BodyEdge, EdgeKind, FunctionBody, LocalTargets};
pub use function_graph::{FunctionGraph, FunctionReference, ReferenceKind};
use std::collections::BTreeMap;
use std::io::Write;
use xbox_recomp2_executable::GuestAddress64;

mod function;
mod function_body;
mod function_graph;

/// Collects the functions and instructions to pass to the generator.
/// See `function-discovery.md` and module header for more details? Okay? :3
pub(super) fn functions(
    config: &RecompilerConfig,
    graph: &DiscoveryGraph,
    diag: &mut dyn Write,
) -> (Vec<Function>, BTreeMap<GuestAddress64, FunctionBody>) {
    let mut functions = Vec::with_capacity(graph.functions.len());
    let mut bodies = BTreeMap::new();
    for (&entry, function) in &graph.functions {
        // First, see if a configured function overrides one we found with analysis.
        if let Some(size) = config.functions.get(FunctionAddress::from(entry.get())) {
            functions.push(Function {
                base: entry.into(),
                size: u64::from(size.0),
            });
            continue;
        }

        // If a function found by brute force starts inside a manually configured
        // function, skip it unless we also found it another way.
        //
        // Example:
        //
        // ```text
        // 0x1000             0x1010               0x1020
        // [--------------------|--------------------)
        //                      ^                    ^
        //               scan-only: skip       outside the range
        // ```
        //
        // Manually configured function: 0x1000, size 0x20 bytes.
        // Found function: Starts at 0x1010.
        // Therefore we keep the manual one.
        let found_only_by_brute_force = function
            .discovery_sources
            .iter()
            .all(|source| matches!(source, FunctionDiscoverySource::BruteForce { .. }));

        let address = entry.get();
        let starts_in_configured_range = config.functions.iter().any(|(start, size)| {
            // First check that we're not before the function.
            // Then check we're not beyond the end of it.
            address >= start.0 && address - start.0 < size.0
        });

        if found_only_by_brute_force && starts_in_configured_range {
            continue;
        }

        if !function.instructions.contains_key(&entry) {
            let _ = writeln!(
                diag,
                concat!(
                    "ERROR: Analysis found no instruction at function entry 0x{:X}; ",
                    "the generated function will abort if called. ",
                    "Verify the entry address and function size in a disassembler. ",
                    "If this is a valid function, add its address and size in bytes ",
                    "to `fixes.functions`."
                ),
                entry.get()
            );
        }

        // Okay. Now store the funcions
        let end = function.decoded_end().unwrap_or(u64::from(entry.get()));
        functions.push(Function {
            base: entry.into(),
            size: end - u64::from(entry.get()),
        });

        let mut edges = Vec::new();
        for block in &function.blocks {
            // Connect each instruction to the next one in this block.
            edges.extend(block.instructions.windows(2).map(|pair| BodyEdge {
                source: pair[0],
                target: pair[1],
                kind: EdgeKind::NextInstruction,
            }));

            // Copy the paths leaving the block's last instruction.
            edges.extend(block.successors.iter().map(|edge| BodyEdge {
                source: edge.source,
                target: edge.target,
                kind: edge.kind.into(),
            }));
        }

        // For branches with.
        // A hook can return or jump away before the original branch runs.
        //
        // We still write that branch into the C++, so its goto needs a label
        // for the C++ to compile.
        for instruction in function.instructions.values() {
            // Check whether this instruction has a hook.
            let has_hook = config
                .midasm_hooks
                .get(FunctionAddress::from(instruction.address.get()))
                .is_some();

            if !has_hook {
                continue;
            }

            // Check whether it's a jump or conditional branch.
            let is_branch = matches!(instruction.flow, Flow::Jump | Flow::Conditional);
            if !is_branch {
                continue;
            }

            // Get the branch's target address.
            let Some(branch_target) = instruction.direct_target() else {
                continue;
            };

            // Add the label edge if the target belongs to this function.
            if function.instructions.contains_key(&branch_target) {
                edges.push(BodyEdge {
                    source: instruction.address,
                    target: branch_target,
                    kind: EdgeKind::Jump,
                });
            }

            // TODO: There may be a cleaner way to represent this.
            //       See if there's a way down the road.
        }

        // Now add the new function.
        bodies.insert(
            GuestAddress64::from(entry),
            FunctionBody::new(
                entry,
                function
                    .instructions
                    .values()
                    .map(|instruction| (instruction.address, instruction.length)),
                edges,
            ),
        );
    }
    (functions, bodies)
}

// TODO: Add tests here.
