//! Check what happens when saved jump targets change a function body.
//!
//! Assembly snippets start at 0x1000; `at(n)` is n bytes past the entry.

use super::*;
use crate::analysis::graph::{TransferKind, UnresolvedTransfer};

// ── Core behaviour ───────────────────────────────────────────────────────────

/// Saved edges extend the body only when the walk reaches their source.
#[rstest]
#[case::direct(&[0xeb, 2, 0xcc, 0xcc], EdgeKind::Jump)]
#[case::conditional(&[0x74, 2, 0xc3, 0xcc], EdgeKind::BranchTaken)]
#[case::switch(&[0xff, 0xe0, 0xcc, 0xcc], EdgeKind::SwitchCase)]
fn rewalk_should_follow_only_reachable_findings(#[case] jump_bytes: &[u8], #[case] kind: EdgeKind) {
    // Arrange
    // Start without saved targets, then add findings at reachable and missed sites.
    // ```text
    // direct:
    // 0x1000: EB 02           jmp  0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // conditional:
    // 0x1000: 74 02           je   0x1004
    // 0x1002: C3              ret               ; not-taken path stays local
    // 0x1003: CC              int3              ; skipped padding
    //
    // switch:
    // 0x1000: FF E0           jmp  eax          ; later learns target 0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // Every case ends with:
    // 0x1004: FF E0           jmp  eax          ; later learns edge 0x1006, reference 0x1007
    // 0x1006: C3              ret               ; reached through the saved edge
    // 0x1007: C3              ret               ; referenced, never walked here
    // 0x1008: CC              int3              ; target of unreachable source 0x1064
    // ```
    let mut code = jump_bytes.to_vec();
    code.extend_from_slice(&[0xff, 0xe0, 0xc3, 0xc3, 0xcc]);
    let image = executable(&code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk before the resolver adds targets.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The first walk cannot reach 0x1006.
    assert!(!graph.functions[&base()].instructions.contains_key(&at(6)));

    // Arrange
    // Save a local path through 0x1004 and a reference to 0x1007.
    seed_resolver_edge(&mut graph, kind, base(), at(4));
    let learned_local_edge = LocalEdge {
        source: at(4),
        target: at(6),
        kind: EdgeKind::SwitchCase,
    };
    add_local_edge(&mut graph, base(), learned_local_edge);

    // The edge from 0x1064 must not pull 0x1008 into this body.
    add_local_edge(
        &mut graph,
        base(),
        LocalEdge {
            source: at(100),
            target: at(8),
            kind: EdgeKind::SwitchCase,
        },
    );
    let learned_tail_call = FunctionReference {
        site: at(4),
        target: at(7),
        kind: ReferenceKind::TailCall,
    };
    graph.add_reference(base(), learned_tail_call);

    // Act
    // Rewalk with the saved findings.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // Follow the saved local path, not the reference or the missed edge.
    // The conditional jump also keeps its not-taken path.
    let body = &graph.functions[&base()];
    let has_fallthrough = kind == EdgeKind::BranchTaken;
    let mut expected_instructions = BTreeSet::from([base(), at(4), at(6)]);
    if has_fallthrough {
        expected_instructions.insert(at(2));
    }
    assert_eq!(
        body.instructions.keys().copied().collect::<BTreeSet<_>>(),
        expected_instructions
    );
    assert!(body.block(base()).unwrap().successors.contains(&LocalEdge {
        source: base(),
        target: at(4),
        kind,
    }));
    assert_eq!(body.references, BTreeSet::from([learned_tail_call]));
}

// Finding a target does not resolve every destination of an indirect jump.
#[rstest]
#[case::direct(&[0xeb, 2, 0xcc, 0xcc], EdgeKind::Jump, &[4])]
#[case::conditional(&[0x74, 2, 0xc3, 0xcc], EdgeKind::BranchTaken, &[4])]
#[case::switch(&[0xff, 0xe0, 0xcc, 0xcc], EdgeKind::SwitchCase, &[0, 4])]
fn rewalk_should_keep_indirect_jumps_unresolved_after_learning_targets(
    #[case] jump_bytes: &[u8],
    #[case] kind: EdgeKind,
    #[case] unresolved_offsets: &[u32],
) {
    // Arrange
    // All cases reach an indirect jump at 0x1004; two start with one too.
    // ```text
    // direct:
    // 0x1000: EB 02           jmp  0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // conditional:
    // 0x1000: 74 02           je   0x1004
    // 0x1002: C3              ret
    // 0x1003: CC              int3              ; skipped padding
    //
    // switch:
    // 0x1000: FF E0           jmp  eax          ; learns local target 0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // Every case ends with:
    // 0x1004: FF E0           jmp  eax          ; learns edge 0x1006, reference 0x1007
    // 0x1006: C3              ret
    // 0x1007: C3              ret
    // ```
    let mut code = jump_bytes.to_vec();
    code.extend_from_slice(&[0xff, 0xe0, 0xc3, 0xc3]);
    let image = executable(&code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk before learning targets.
    walker.walk_function(&mut graph, base()).unwrap();

    // Arrange
    // Save targets for both the entry jump and the jump at 0x1004.
    seed_resolver_edge(&mut graph, kind, base(), at(4));
    seed_resolver_edge(&mut graph, EdgeKind::SwitchCase, at(4), at(6));
    graph.add_reference(
        base(),
        FunctionReference {
            site: at(4),
            target: at(7),
            kind: ReferenceKind::TailCall,
        },
    );

    // Act
    // Rewalk with the new findings.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The targets are usable, but each indirect jump remains unresolved.
    let body = &graph.functions[&base()];
    assert!(body.instructions.contains_key(&at(6)));
    assert_eq!(
        body.unresolved,
        unresolved_offsets
            .iter()
            .map(|&offset| UnresolvedTransfer {
                site: at(offset),
                kind: TransferKind::Jump,
            })
            .collect::<BTreeSet<_>>()
    );
}

// Making 0x1004 a separate function removes its path, not its saved findings.
#[rstest]
#[case::direct(&[0xeb, 2, 0xcc, 0xcc], EdgeKind::Jump)]
#[case::conditional(&[0x74, 2, 0xc3, 0xcc], EdgeKind::BranchTaken)]
#[case::switch(&[0xff, 0xe0, 0xcc, 0xcc], EdgeKind::SwitchCase)]
fn rewalk_should_keep_saved_findings_when_a_local_target_becomes_a_function(
    #[case] jump_bytes: &[u8],
    #[case] kind: EdgeKind,
) {
    // Arrange
    // Start with a jump to 0x1004, which is still part of this function.
    // ```text
    // direct:
    // 0x1000: EB 02           jmp  0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // conditional:
    // 0x1000: 74 02           je   0x1004
    // 0x1002: C3              ret               ; not-taken path stays local
    // 0x1003: CC              int3              ; skipped padding
    //
    // switch:
    // 0x1000: FF E0           jmp  eax          ; saved target: 0x1004
    // 0x1002: CC CC           int3; int3        ; skipped padding
    //
    // Every case ends with:
    // 0x1004: FF E0           jmp  eax          ; saved edge 0x1006, reference 0x1007
    // 0x1006: C3              ret               ; leaves the body with 0x1004
    // 0x1007: C3              ret               ; referenced, never walked here
    // ```
    let mut code = jump_bytes.to_vec();
    code.extend_from_slice(&[0xff, 0xe0, 0xc3, 0xc3]);
    let image = executable(&code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk without findings.
    walker.walk_function(&mut graph, base()).unwrap();

    // Arrange
    // Add a local path from 0x1004 and a reference from it to 0x1007.
    seed_resolver_edge(&mut graph, kind, base(), at(4));
    let learned_local_edge = LocalEdge {
        source: at(4),
        target: at(6),
        kind: EdgeKind::SwitchCase,
    };
    let learned_tail_call = FunctionReference {
        site: at(4),
        target: at(7),
        kind: ReferenceKind::TailCall,
    };
    add_local_edge(&mut graph, base(), learned_local_edge);
    graph.add_reference(base(), learned_tail_call);

    // Act
    // Follow the saved path.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // 0x1006 is local and 0x1007 remains a reference.
    assert!(graph.functions[&base()].instructions.contains_key(&at(6)));
    assert!(
        graph.functions[&base()]
            .references
            .contains(&learned_tail_call)
    );

    // Arrange
    // Mark 0x1004 as a separate function.
    graph.add_function(at(4), FunctionDiscoverySource::Symbol, None);

    // Act
    // Rebuild the original body.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The jump to 0x1004 becomes a tail call, removing 0x1004 and 0x1006 from
    // this body. The conditional's not-taken path should stay local.
    let body = &graph.functions[&base()];
    let expected_remaining_instructions = if kind == EdgeKind::BranchTaken {
        vec![base(), at(2)]
    } else {
        vec![base()]
    };
    assert_eq!(
        body.instructions.keys().copied().collect::<Vec<_>>(),
        expected_remaining_instructions
    );
    assert_eq!(
        body.references,
        BTreeSet::from([FunctionReference {
            site: base(),
            target: at(4),
            kind: ReferenceKind::TailCall,
        }])
    );
    assert!(
        body.block(base())
            .unwrap()
            .successors
            .iter()
            .all(|edge| edge.target != at(4))
    );
    // Keep the saved targets even though their source has left the body.
    assert!(body.learned.edges.contains(&learned_local_edge));
    assert!(body.learned.references.contains(&learned_tail_call));
}

// Reuse the entry instruction while a learned path first joins, then leaves.
#[test]
fn rewalk_should_reuse_cached_instructions_when_the_body_changes() {
    // Arrange
    // Start with an unresolved jump to a return that later becomes a function.
    // ```text
    // 0x1000: FF E0           jmp  eax          ; learns target 0x1002
    // 0x1002: C3              ret               ; later becomes a function entry
    // ```
    let image = executable(&[0xff, 0xe0, 0xc3]);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Decode the entry on the first walk.
    walker.walk_function(&mut graph, base()).unwrap();
    let cached_entry = graph.functions[&base()].instructions[&base()].clone();

    // Arrange
    // Add the learned local target.
    seed_resolver_edge(&mut graph, EdgeKind::SwitchCase, base(), at(2));

    // Act
    // Rewalk after learning the target.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The body grows, but the entry instruction uses the same cached value.
    let body = &graph.functions[&base()];
    assert!(body.instructions.contains_key(&at(2)));
    assert!(Arc::ptr_eq(&cached_entry, &body.instructions[&base()]));

    // Arrange
    // Give the target its own function entry.
    graph.add_function(at(2), FunctionDiscoverySource::Symbol, None);

    // Act
    // Rewalk the original function.
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The body shrinks, but the cached entry instruction stays the same.
    let body = &graph.functions[&base()];
    assert!(!body.instructions.contains_key(&at(2)));
    assert!(Arc::ptr_eq(&cached_entry, &body.instructions[&base()]));
}
