//! Tells passes whether we know every caller of a function.
//!
//! See `known-callers.md` for more info.
//!
//! - [`retain`]: keeps candidates whose every caller we can rewrite.
//! - [`direct_call_target`]: returns the target of a `call rel32`.

use crate::analysis::{BruteForceKind, DiscoveryReferenceKind, FunctionDiscoverySource};
use crate::analysis::{FunctionGraph, FunctionReference, ReferenceKind};
use crate::codegen::function::model::{FunctionModel, GuestOp, Op};
use crate::config::FunctionAddress;
use crate::decode::{BranchType, Mnemonic, immediate};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// Returns the target of a near `call rel32`, or [`None`] for anything else.
pub(in crate::codegen) fn direct_call_target(guest: &GuestOp) -> Option<FunctionAddress> {
    let decoded = guest.insn.instruction();
    if guest.insn.mnemonic() != Mnemonic::CALL
        || decoded.operand_width != 32
        || decoded.meta.branch_type != BranchType::NEAR
    {
        return None;
    }

    // `rel32` counts from the end of the instruction, and wraps around.
    let offset = immediate(&guest.insn.operands()[0]).filter(|offset| offset.is_relative)?;
    let target = guest
        .address
        .wrapping_add(guest.insn.length() as u32)
        .wrapping_add(offset.value as u32);
    Some(FunctionAddress(target.get()))
}

/// Keeps only candidates whose every caller is a direct `call` we can rewrite.
///
/// # Arguments
///
/// - `candidates`: the pass's data for each function it wants to change.
/// - `models`: every function model.
/// - `graph`: the references discovery found.
/// - `accept`: the pass's check for one call, given `(candidate, ops, call index)`.
///   It may update the candidate.
///
/// # Remarks
///
/// This only checks callers.
/// Check the candidates first, e.g. with [`FunctionModel::can_change_abi`].
pub(in crate::codegen) fn retain<T>(
    candidates: &mut BTreeMap<FunctionAddress, T>,
    models: &[FunctionModel],
    graph: &FunctionGraph,
    mut accept: impl FnMut(&mut T, &[Op], usize) -> bool,
) {
    let models: BTreeMap<_, _> = models.iter().map(|model| (model.address, model)).collect();
    candidates.retain(|target, candidate| {
        let address = target.0.into();

        // Every reference must be a direct `call` we can rewrite.
        // i.e. Verify we can rewrite all `callers`.
        //
        // i.e. no tail-call `jmp`s, or possible pointers like `push 0x20000`.
        let incoming = graph.incoming(address); // calls to rewrite
        for reference in incoming {
            // Remove any non `call`
            if reference.kind != ReferenceKind::DirectCall {
                return false;
            }

            let Some(model) = models.get(&FunctionAddress(reference.source.get())) else {
                return false;
            };

            // Fetch call for external validator via accept()
            let Some((ops, index)) = find_call(model, reference.site) else {
                return false;
            };

            // Protected? (No ABI change). Yeet it out.
            if !model.can_change_abi() || !accept(candidate, ops, index) {
                return false;
            }
        }

        // If anything other than a traceable `call` we can see led us here,
        // there may be other callers. Then we can't safely break ABI.
        found_only_by_code(address, graph, false)
    });
}

/// Whether discovery found `function` only through code it walked.
/// And not any brute-force strategies or unknown sources.
pub(in crate::codegen) fn found_only_by_code(
    function: GuestAddress32,
    graph: &FunctionGraph,
    tail_calls: bool,
) -> bool {
    let incoming = graph.incoming(function);
    let Some(sources) = graph.discovery_sources(function) else {
        return false;
    };
    !incoming.is_empty()
        && sources
            .iter()
            .all(|source| is_code_source(source, incoming, tail_calls))
}

/// Whether discovery found a function through code it walked.
///
/// # Arguments
///
/// - `source`: one way discovery found the function.
/// - `incoming`: the function's references, from [`FunctionGraph::incoming`].
/// - `tail_calls`: whether a tail jump counts.
pub(in crate::codegen) fn is_code_source(
    source: &FunctionDiscoverySource,
    incoming: &[FunctionReference],
    tail_calls: bool,
) -> bool {
    let walked = |site: &GuestAddress32| incoming.iter().any(|reference| reference.site == *site);
    match source {
        FunctionDiscoverySource::Reference {
            kind: DiscoveryReferenceKind::DirectCall,
            ..
        } => true,
        FunctionDiscoverySource::Reference {
            kind: DiscoveryReferenceKind::TailCall,
            ..
        } => tail_calls,
        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::DirectCall { site },
        } => walked(site),
        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::TailCall { site },
        } => tail_calls && walked(site),

        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::Padding,
        } => true,

        _ => false,
    }
}

/// Finds the `call rel32` at `site` we can rewrite.
/// Returns block ops and index.
///
/// Returns [`None`] for other calls types, like the 16-bit boys.
fn find_call(model: &FunctionModel, site: GuestAddress32) -> Option<(&[Op], usize)> {
    for block in &model.blocks {
        for (index, op) in block.ops.iter().enumerate() {
            let Op::Call(call) = op else { continue };
            if call.guest.address != site {
                continue;
            }

            let rewritable =
                direct_call_target(&call.guest).is_some() && !call.guest.last_in_function;
            return rewritable.then_some((&block.ops, index));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::{executable, layout, program};
    use crate::config::{FunctionSize, RecompilerConfig};
    use crate::executable::{Section, SectionFlags};
    use rstest::rstest;

    /// Another way the program reaches 0x1040, besides direct calls.
    #[derive(Clone, Copy)]
    enum OtherReference {
        /// No other way.
        None,

        /// A `.data` section at 0x2000 holds 0x1040.
        StoredPointer,

        /// The config lists 0x1040 as a function.
        ConfiguredFunction,
    }

    // ── Core behavior ────────────────────────────────────────────────────────

    /// 0x1000 calls 0x1040. Each case adds more code at 0x101F.
    #[rstest]
    #[case::second_call(
        &[
            0xCC,                   // int3                 ; padding: a function starts after it
            0x52,                   // push edx
            0xE8, 0x1A, 0, 0, 0,    // call 0x1040
            0xC3,                   // ret
        ],
        OtherReference::None,
        true,
    )]
    #[case::unknown_caller(
        &[
            0x90,                   // nop                  ; <- so no function starts after it
            0x52,                   // push edx
            0xE8, 0x1A, 0, 0, 0,    // call 0x1040
            0xC3,                   // ret
        ],
        OtherReference::None,
        false,
    )]
    #[case::tail_jump(
        &[
            0xCC,                   // int3
            0xE9, 0x1B, 0, 0, 0,    // jmp  0x1040          ; <- not a call
        ],
        OtherReference::None,
        false,
    )]
    #[case::callback(
        &[
            0xCC,                   // int3
            0x68, 0x40, 0x10, 0, 0, // push 0x1040          ; <- maybe a callback
            0xC3,                   // ret
        ],
        OtherReference::None,
        false,
    )]
    #[case::stored_pointer(
        &[
            0xCC,                   // int3
            0xB8, 0, 0x20, 0, 0,    // mov  eax, 0x2000
            0xC3,                   // ret
        ],
        OtherReference::StoredPointer,      // <- 0x2000 holds 0x1040
        false,
    )]
    #[case::configured_function(
        &[
            0xCC,                   // int3
            0xC3,                   // ret
        ],
        OtherReference::ConfiguredFunction, // <- not found through a call
        false,
    )]
    fn retain_should_keep_only_functions_with_known_callers(
        #[case] second: &[u8],
        #[case] other_reference: OtherReference,
        #[case] expected: bool,
    ) {
        // Arrange
        #[rustfmt::skip]
        let code = layout(&[
            (0x1000, &[
                0x52,                   // push edx
                0xE8, 0x3A, 0, 0, 0,    // call 0x1040
                0xC3,                   // ret
            ]),
            (0x101F, second),
            (0x1040, &[
                0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]
                0x89, 0x01,             // mov  [ecx], eax
                0xC2, 4, 0,             // ret  4
            ]),
        ]);
        let mut executable = executable(&code);
        let mut config = RecompilerConfig::default();
        match other_reference {
            OtherReference::None => {}
            OtherReference::StoredPointer => {
                let offset = executable.data.len() as u32;
                let data = Section::new(".data", 0x2000, 5, SectionFlags::DATA, offset);
                executable.sections.push(data);
                executable.data.extend_from_slice(&[0x40, 0x10, 0, 0, 0]);
                executable.size = executable.data.len() as u32;
            }
            OtherReference::ConfiguredFunction => {
                let size = FunctionSize(9);
                config
                    .functions
                    .insert_or_overwrite(FunctionAddress(0x1040), size);
            }
        }
        let (graph, models) = program(&mut executable, &config);
        let mut candidates = BTreeMap::from([(FunctionAddress(0x1040), ())]);

        // Act
        retain(&mut candidates, &models, &graph, |_, _, _| true);

        // Assert
        assert_eq!(candidates.contains_key(&FunctionAddress(0x1040)), expected);
    }
}
