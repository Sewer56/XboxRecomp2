//! Check how direct calls add and retain function references.
//!
//! Assembly snippets start at 0x1000; `at(n)` is n bytes past the entry.

use super::*;
use crate::analysis::BruteForceKind;
use crate::analysis::finalize::FunctionGraph;

// ── Core behaviour ───────────────────────────────────────────────────────────

/// Rebuilding either function must not drop or duplicate the caller's reference.
#[rstest]
#[case::recursive(&[0xe8, 0xfb, 0xff, 0xff, 0xff, 0xc3], BASE, false)]
#[case::new_callee(&[0xe8, 1, 0, 0, 0, 0xc3, 0xc3], BASE + 6, false)]
#[case::fallback_first(&[0xe8, 1, 0, 0, 0, 0xc3, 0xc3], BASE + 6, true)]
fn rewalk_should_preserve_call_references_in_both_indexes(
    #[case] code: &[u8],
    #[case] callee: u32,
    #[case] scan_callee_first: bool,
) {
    // Arrange
    // Cover recursion, a new callee, and a callee already found by a scan.
    // ```text
    // recursive:
    // 0x1000: E8 FB FF FF FF  call 0x1000
    // 0x1005: C3              ret
    //
    // new_callee, fallback_first:
    // 0x1000: E8 01 00 00 00  call 0x1006
    // 0x1005: C3              ret               ; caller ends here
    // 0x1006: C3              ret               ; separate callee
    // ```
    let image = executable(code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    let callee_address = GuestAddress32::from(callee);
    if scan_callee_first {
        graph.add_function(
            callee_address,
            FunctionDiscoverySource::BruteForce {
                kind: BruteForceKind::Padding,
            },
            None,
        );
        walker.walk_function(&mut graph, callee_address).unwrap();
    }
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk the caller, then walk it again after any scan-found callee.
    walker.walk_function(&mut graph, base()).unwrap();

    if scan_callee_first {
        walker.walk_function(&mut graph, callee_address).unwrap();
    }
    let repeated_walk = walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // Neither rewalking nor finalising the graph drops or duplicates the call.
    assert!(!repeated_walk.changed);
    assert!(repeated_walk.new_functions.is_empty());
    assert_eq!(
        graph.functions[&base()].references,
        BTreeSet::from([FunctionReference {
            site: base(),
            target: callee_address,
            kind: ReferenceKind::DirectCall,
        }])
    );

    let references = FunctionGraph::from_discovery(graph);
    assert_eq!(
        references.incoming(callee.into()),
        references.outgoing(BASE.into())
    );
    assert_eq!(references.incoming(callee.into()).len(), 1);
    assert_eq!(references.incoming(callee.into())[0].site, BASE.into());
    assert_eq!(
        references.incoming(callee.into())[0].kind,
        crate::analysis::ReferenceKind::DirectCall
    );
}

// A call adds a function reference, then continues at the caller's ret.
#[rstest]
#[case::recursive(&[0xe8, 0xfb, 0xff, 0xff, 0xff, 0xc3], BASE, &[])]
#[case::new_callee(&[0xe8, 1, 0, 0, 0, 0xc3, 0xc3], BASE + 6, &[BASE + 6])]
fn walk_should_add_call_references_without_walking_the_callee(
    #[case] code: &[u8],
    #[case] callee: u32,
    #[case] expected_new_functions: &[u32],
) {
    // Arrange
    // Compare a recursive call with a call to a separate, unknown callee.
    // ```text
    // recursive:
    // 0x1000: E8 FB FF FF FF  call 0x1000
    // 0x1005: C3              ret
    //
    // new_callee:
    // 0x1000: E8 01 00 00 00  call 0x1006
    // 0x1005: C3              ret               ; caller ends here
    // 0x1006: C3              ret               ; separate callee
    // ```
    let image = executable(code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    let callee_address = GuestAddress32::from(callee);
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk the caller, then walk it again without walking the callee.
    let first_walk = walker.walk_function(&mut graph, base()).unwrap();
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // Report a new function only if the callee was unknown.
    assert!(first_walk.changed);
    assert_eq!(
        first_walk.new_functions,
        expected_new_functions
            .iter()
            .copied()
            .map(GuestAddress32::from)
            .collect::<Vec<_>>()
    );

    // The caller still contains only its call and return.
    let caller = &graph.functions[&base()];
    assert_eq!(
        caller.instructions.keys().copied().collect::<Vec<_>>(),
        [base(), at(5)]
    );
    assert_eq!(caller.blocks.len(), 1);
    assert!(caller.diagnostics.is_empty());
    assert_eq!(
        caller.references,
        BTreeSet::from([FunctionReference {
            site: base(),
            target: callee_address,
            kind: ReferenceKind::DirectCall,
        }])
    );
    assert_eq!(walker.cache.len(), 2);
}

// Finding a caller adds another discovery source; it does not replace the scan.
#[test]
fn walk_should_keep_the_scan_source_when_a_caller_finds_the_same_callee() {
    // Arrange
    // Walk the scan-found callee before adding its caller.
    // ```text
    // 0x1000: E8 01 00 00 00  call 0x1006
    // 0x1005: C3              ret               ; caller ends here
    // 0x1006: C3              ret               ; scan finds this function first
    // ```
    let image = executable(&[0xe8, 1, 0, 0, 0, 0xc3, 0xc3]);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&image, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(
        at(6),
        FunctionDiscoverySource::BruteForce {
            kind: BruteForceKind::Padding,
        },
        None,
    );
    walker.walk_function(&mut graph, at(6)).unwrap();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

    // Act
    // Walk the caller and rebuild both functions.
    let first_walk = walker.walk_function(&mut graph, base()).unwrap();
    walker.walk_function(&mut graph, at(6)).unwrap();
    walker.walk_function(&mut graph, base()).unwrap();

    // Assert
    // The call adds a discovery source without replacing the scan source.
    assert!(first_walk.changed);
    assert!(first_walk.new_functions.is_empty());
    assert_eq!(
        graph.functions[&at(6)].discovery_sources,
        BTreeSet::from([
            FunctionDiscoverySource::BruteForce {
                kind: BruteForceKind::Padding
            },
            FunctionDiscoverySource::Reference {
                function: base(),
                site: base(),
                kind: ReferenceKind::DirectCall,
            },
        ])
    );

    // The caller keeps its own body, even though the scan walked the callee first.
    let caller = &graph.functions[&base()];
    assert_eq!(
        caller.instructions.keys().copied().collect::<Vec<_>>(),
        [base(), at(5)]
    );
    assert_eq!(caller.blocks.len(), 1);
    assert!(caller.diagnostics.is_empty());
    assert_eq!(walker.cache.len(), 3);
}
