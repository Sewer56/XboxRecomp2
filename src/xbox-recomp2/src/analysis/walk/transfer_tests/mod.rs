//! Check entry loops, configured range boundaries, and converging paths.
//!
//! Assembly snippets start at 0x1000. `base()` is that address; `at(n)` is
//! n bytes after it.
//!
//! TODO: Clean this module - Sewer
//!       This still some remnants of initial prototyping. It sux.
//!       And while each test is validated by hand.
//!       Some could be made easier to follow.
use super::*;
use crate::analysis::graph::{
    FunctionReference, ReferenceKind,
    test_helpers::{BASE, add_local_edge, at, base, executable},
};
use crate::config::FunctionSize;
use rstest::rstest;

mod call_tests;
mod rewalk_tests;

// ── Core behaviour ───────────────────────────────────────────────────────────

/// Use identical bytes to isolate the effect of an explicit function size.
#[rstest]
#[case::direct(&[0x90, 0xeb, 0xfd], EdgeKind::Jump, None)]
#[case::configured_direct(&[0x90, 0xeb, 0xfd], EdgeKind::Jump, Some(3))]
#[case::conditional(&[0x74, 0xfe, 0xc3], EdgeKind::BranchTaken, None)]
#[case::configured_conditional(&[0x74, 0xfe, 0xc3], EdgeKind::BranchTaken, Some(3))]
#[case::switch(&[0xff, 0xe0], EdgeKind::SwitchCase, None)]
#[case::configured_switch(&[0xff, 0xe0], EdgeKind::SwitchCase, Some(2))]
fn walk_should_loop_locally_unless_the_entry_has_a_configured_size(
    #[case] code: &[u8],
    #[case] kind: EdgeKind,
    #[case] configured_size: Option<u32>,
) {
    // Arrange
    // Pair the same bytes with and without a configured size. Without a size,
    // jumping to the entry stays local; an explicit size makes it a tail call.
    //
    // ```text
    // direct (no size), configured_direct (3 bytes):
    // 0x1000: 90              nop
    // 0x1001: EB FD           jmp  0x1000
    //
    // conditional (no size), configured_conditional (3 bytes):
    // 0x1000: 74 FE           je   0x1000
    // 0x1002: C3              ret
    //
    // switch (no size), configured_switch (2 bytes):
    // 0x1000: FF E0           jmp  eax          ; saved switch case: 0x1000
    // ```
    let image = executable(code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(
        base(),
        FunctionDiscoverySource::EntryPoint,
        configured_size.map(FunctionSize),
    );
    seed_resolver_edge(&mut graph, kind, base(), base());

    // Act
    // Walk the entry with the case's size setting.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // Both size settings decode the same instructions and ordered blocks.
    let body = &graph.functions[&base()];
    let expected_offsets: &[u32] = match kind {
        EdgeKind::Jump => &[0, 1],
        EdgeKind::BranchTaken => &[0, 2],
        _ => &[0],
    };
    assert_eq!(
        body.instructions
            .keys()
            .map(|address| address.get() - BASE)
            .collect::<Vec<_>>(),
        expected_offsets
    );
    assert_eq!(
        body.blocks.len(),
        if kind == EdgeKind::BranchTaken { 2 } else { 1 }
    );
    assert!(
        body.blocks
            .windows(2)
            .all(|pair| pair[0].start < pair[1].start)
    );
    assert!(body.diagnostics.is_empty());

    // An explicit size replaces the local loop with a tail-call reference.
    let expects_tail_call = configured_size.is_some();
    assert_eq!(body.references.len(), usize::from(expects_tail_call));
    assert_eq!(
        body.block(base())
            .unwrap()
            .successors
            .iter()
            .any(|edge| edge.target == base()),
        !expects_tail_call
    );
}

// A known entry stays local only when it lies strictly inside the configured body.
#[rstest]
#[case::direct(&[0xeb, 2, 0xcc, 0xcc, 0xc3], EdgeKind::Jump, 5, true)]
#[case::conditional(&[0x74, 2, 0xc3, 0xcc, 0xc3], EdgeKind::BranchTaken, 5, true)]
#[case::switch(&[0xff, 0xe0, 0xcc, 0xcc, 0xc3], EdgeKind::SwitchCase, 5, true)]
#[case::at_end(&[0xeb, 2, 0xcc, 0xcc, 0xc3], EdgeKind::Jump, 4, false)]
#[case::wrapped_end(&[0xeb, 2, 0xcc, 0xcc, 0xc3], EdgeKind::Jump, u32::MAX, false)]
fn walk_should_match_configured_range_boundaries(
    #[case] code: &[u8],
    #[case] kind: EdgeKind,
    #[case] configured_size: u32,
    #[case] expect_local_target: bool,
) {
    // Arrange
    // 0x1004 is a known entry inside the range, at its end, or beyond a wrapped end.
    // ```text
    // direct, at_end, wrapped_end:
    // 0x1000: EB 02           jmp  0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // conditional:
    // 0x1000: 74 02           je   0x1004
    // 0x1002: C3              ret
    // 0x1003: CC              int3              ; skipped padding
    //
    // switch:
    // 0x1000: FF E0           jmp  eax          ; saved switch case: 0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // Shared destination:
    // 0x1004: C3              ret               ; known function entry
    //
    // Sizes are in bytes.
    // These ends use the wrapping calculation for jumps:
    //
    // direct/conditional/switch: size 5,          end 0x1005, target is inside
    // at_end:                    size 4,          end 0x1004, target is outside
    // wrapped_end:               size 0xFFFFFFFF, end 0x0FFF, target is outside
    // ```
    // The decoder checks its size limit separately, without wrapping the end.
    let image = executable(code);
    let mut config = RecompilerConfig::default();
    config
        .functions
        .insert_or_overwrite(BASE.into(), FunctionSize(configured_size));
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::Manual, None);
    graph.add_function(at(4), FunctionDiscoverySource::Symbol, None);
    seed_resolver_edge(&mut graph, kind, base(), at(4));

    // Act
    // Classify the transfer against the configured range.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // Follow interior targets locally; add the others as references.
    let body = &graph.functions[&base()];
    assert_eq!(body.instructions.contains_key(&at(4)), expect_local_target);
    assert_eq!(body.references.is_empty(), expect_local_target);
    assert_eq!(
        body.block(base()).unwrap().successors.contains(&LocalEdge {
            source: base(),
            target: at(4),
            kind,
        }),
        expect_local_target
    );
}

// A second path to a known entry makes direct branches and switch cases local.
#[rstest]
#[case::direct(&[0xeb, 1], EdgeKind::Jump)]
#[case::conditional(&[0x74, 1], EdgeKind::BranchTaken)]
#[case::switch(&[0xff, 0xe0], EdgeKind::SwitchCase)]
fn walk_should_restore_local_edges_when_another_path_reaches_the_entry(
    #[case] transfer_bytes: &[u8],
    #[case] kind: EdgeKind,
) {
    // Arrange
    // The nop path and the transfer both lead to the known entry.
    // ```text
    // Every case starts with:
    // 0x1000: 85 C0           test eax, eax
    // 0x1002: 74 02           je   0x1006
    //
    // At 0x1004, choose one transfer:
    // 0x1004: EB 01           jmp  0x1007       ; direct
    // 0x1004: 74 01           je   0x1007       ; conditional
    // 0x1004: FF E0           jmp  eax          ; switch: saved edge to 0x1007
    //
    // Every case ends with:
    // 0x1006: 90              nop
    // 0x1007: C3              ret               ; also reached through the nop
    // ```
    let mut code = vec![0x85, 0xc0, 0x74, 2];
    code.extend_from_slice(transfer_bytes);
    code.extend_from_slice(&[0x90, 0xc3]);
    let image = executable(&code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);
    graph.add_function(at(7), FunctionDiscoverySource::Symbol, None);
    seed_resolver_edge(&mut graph, kind, at(4), at(7));

    // Act
    // Walk both paths to the known entry.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The nop reaches 0x1007, so this body includes the ret.
    let body = &graph.functions[&base()];
    assert_eq!(
        body.instructions.keys().copied().collect::<Vec<_>>(),
        [base(), at(2), at(4), at(6), at(7)]
    );
    assert_eq!(
        body.blocks
            .iter()
            .map(|block| block.start)
            .collect::<Vec<_>>(),
        [base(), at(4), at(6), at(7)]
    );
    assert!(body.diagnostics.is_empty());

    // The transfer at 0x1004 also has a local edge to 0x1007.
    assert!(body.references.is_empty());
    assert!(body.block(at(4)).unwrap().successors.contains(&LocalEdge {
        source: at(4),
        target: at(7),
        kind,
    }));
}

/// Save a switch edge in the function at `base()`.
///
/// Direct and conditional branches encode their targets in the instruction bytes.
fn seed_resolver_edge(
    graph: &mut DiscoveryGraph,
    kind: EdgeKind,
    source: GuestAddress32,
    target: GuestAddress32,
) {
    if kind == EdgeKind::SwitchCase {
        add_local_edge(
            graph,
            base(),
            LocalEdge {
                source,
                target,
                kind,
            },
        );
    }
}
