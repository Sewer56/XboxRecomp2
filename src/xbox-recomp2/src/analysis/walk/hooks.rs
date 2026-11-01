//! Account for hooks that change state or redirect control flow.

use super::branches;
use crate::analysis::graph::{EdgeKind, FunctionNode, Instruction, LearnedTargets, LocalEdge};
use crate::config::MidAsmHook;
use std::collections::BTreeMap;
use std::sync::Arc;
use xbox_recomp2_executable::GuestAddress32;

/// Paths that remain after a hook runs.
pub(super) struct HookFlow {
    /// Jump targets in generated branch order.
    pub(super) jumps: Vec<GuestAddress32>,

    /// Whether any path still runs the hooked instruction.
    pub(super) runs_original: bool,
}

/// What one branch of the hook does.
enum Outcome {
    /// Falls into the hooked instruction.
    Continue,

    /// Returns from the function; it contributes no edge.
    Return,

    /// Jumps to a configured address.
    Jump(GuestAddress32),
}

/// Prevents pointer scanning on the hooked instruction without changing the cache.
///
/// The hook runs before the instruction and may skip it or change its inputs.
pub(super) fn hooked(instruction: &Arc<Instruction>) -> Arc<Instruction> {
    let mut copy = (**instruction).clone();
    copy.clobbers_all = true;
    Arc::new(copy)
}

/// Follows the hook's jumps and the original instruction
/// if the hook lets it run.
///
/// Hook jumps stay local even if they land on a known function entry.
pub(super) fn local_successors(
    node: &mut FunctionNode,
    instruction: &Instruction,
    hook: &MidAsmHook,
    known_entries: &BTreeMap<GuestAddress32, FunctionNode>,
    learned: &LearnedTargets,
) -> Vec<LocalEdge> {
    let hook_flow = flow(hook);
    // Don't treat a hook jump to a known entry as a tail call.
    let mut edges: Vec<_> = hook_flow
        .jumps
        .into_iter()
        .map(|target| LocalEdge {
            source: instruction.address,
            target,
            kind: EdgeKind::Jump,
        })
        .collect();

    // A return or jump from every hook path skips the original instruction.
    if hook_flow.runs_original {
        edges.extend(branches::local_successors(
            node,
            instruction,
            known_entries,
            learned,
        ));
    }
    edges.sort_unstable_by_key(|edge| (edge.target, edge.kind));
    edges.dedup();

    edges
}

/// Finds which hook branches jump or continue to the instruction.
///
/// - Any conditional setting selects the two-branch form.
/// - Otherwise the plain hook has one path.
/// - In either form, return takes priority over jump.
pub(super) fn flow(hook: &MidAsmHook) -> HookFlow {
    // Use a second, inert path for plain hooks so both forms share the loop.
    let conditional = hook.return_on_false
        || hook.return_on_true
        || hook.jump_address_on_false != 0
        || hook.jump_address_on_true != 0;
    let paths = if conditional {
        [
            outcome(hook.return_on_true, hook.jump_address_on_true),
            outcome(hook.return_on_false, hook.jump_address_on_false),
        ]
    } else {
        [outcome(hook.ret, hook.jump_address), Outcome::Return]
    };

    let mut jumps = Vec::new();
    let mut runs_original = false;
    for path in paths {
        match path {
            Outcome::Continue => runs_original = true,
            Outcome::Jump(target) => {
                if !jumps.contains(&target) {
                    jumps.push(target);
                }
            }
            Outcome::Return => {}
        }
    }

    HookFlow {
        jumps,
        runs_original,
    }
}

/// Uses the generated code's return-before-jump order.
fn outcome(returns: bool, jump: u32) -> Outcome {
    if returns {
        Outcome::Return
    } else if jump != 0 {
        Outcome::Jump(GuestAddress32::from(jump))
    } else {
        Outcome::Continue
    }
}

#[cfg(test)]
mod tests {
    // Check how hooks redirect paths and guard pointer scanning.

    use super::*;
    use crate::analysis::graph::{
        DiscoveryGraph, FunctionDiscoverySource, ReferenceKind,
        test_helpers::{BASE, at, base, executable},
    };
    use crate::analysis::walk::FunctionWalker;
    use crate::config::{FunctionAddress, MidAsmHook, RecompilerConfig};
    use crate::decode::Decoder;
    use rstest::rstest;

    // Expected guest addresses and edges for each hook path through the shared body.
    struct ExpectedHookWalk {
        jumps: &'static [u32],
        runs_original: bool,
        instructions: &'static [u32],
        successors: &'static [(u32, EdgeKind)],
        callees: &'static [u32],
        functions: &'static [u32],
    }

    // ── Core behaviour ───────────────────────────────────────────────────────

    #[rstest]
    #[case::void_continues(
        void_hook(false, 0),
        ExpectedHookWalk {
            jumps: &[],
            runs_original: true,
            instructions: &[0x1000, 0x1005],
            successors: &[(0x1005, EdgeKind::NextInstruction)],
            callees: &[0x1008],
            functions: &[0x1000, 0x1005, 0x1006, 0x1008],
        }
    )]
    #[case::void_returns(
        void_hook(true, 0x1006),
        ExpectedHookWalk {
            jumps: &[],
            runs_original: false,
            instructions: &[0x1000],
            successors: &[],
            callees: &[],
            functions: &[0x1000, 0x1005, 0x1006],
        }
    )]
    #[case::void_jumps(
        void_hook(false, 0x1006),
        ExpectedHookWalk {
            jumps: &[0x1006],
            runs_original: false,
            instructions: &[0x1000, 0x1006],
            successors: &[(0x1006, EdgeKind::Jump)],
            callees: &[],
            functions: &[0x1000, 0x1005, 0x1006],
        }
    )]
    #[case::true_branch_jumps(
        conditional_hook(false, 0x1005, false, 0),
        ExpectedHookWalk {
            jumps: &[0x1005],
            runs_original: true,
            instructions: &[0x1000, 0x1005],
            successors: &[(0x1005, EdgeKind::NextInstruction), (0x1005, EdgeKind::Jump)],
            callees: &[0x1008],
            functions: &[0x1000, 0x1005, 0x1006, 0x1008],
        }
    )]
    #[case::false_branch_jumps(
        conditional_hook(false, 0, false, 0x1006),
        ExpectedHookWalk {
            jumps: &[0x1006],
            runs_original: true,
            instructions: &[0x1000, 0x1005, 0x1006],
            successors: &[(0x1005, EdgeKind::NextInstruction), (0x1006, EdgeKind::Jump)],
            callees: &[0x1008],
            functions: &[0x1000, 0x1005, 0x1006, 0x1008],
        }
    )]
    #[case::both_branches_jump(
        conditional_hook(false, 0x1005, false, 0x1006),
        ExpectedHookWalk {
            jumps: &[0x1005, 0x1006],
            runs_original: false,
            instructions: &[0x1000, 0x1005, 0x1006],
            successors: &[(0x1005, EdgeKind::Jump), (0x1006, EdgeKind::Jump)],
            callees: &[],
            functions: &[0x1000, 0x1005, 0x1006],
        }
    )]
    #[case::true_return_beats_jump(
        conditional_hook(true, 0x1005, false, 0),
        ExpectedHookWalk {
            jumps: &[],
            runs_original: true,
            instructions: &[0x1000, 0x1005],
            successors: &[(0x1005, EdgeKind::NextInstruction)],
            callees: &[0x1008],
            functions: &[0x1000, 0x1005, 0x1006, 0x1008],
        }
    )]
    #[case::false_return_beats_jump(
        conditional_hook(false, 0, true, 0x1006),
        ExpectedHookWalk {
            jumps: &[],
            runs_original: true,
            instructions: &[0x1000, 0x1005],
            successors: &[(0x1005, EdgeKind::NextInstruction)],
            callees: &[0x1008],
            functions: &[0x1000, 0x1005, 0x1006, 0x1008],
        }
    )]
    #[case::both_branches_return(
        conditional_hook(true, 0, true, 0),
        ExpectedHookWalk {
            jumps: &[],
            runs_original: false,
            instructions: &[0x1000],
            successors: &[],
            callees: &[],
            functions: &[0x1000, 0x1005, 0x1006],
        }
    )]
    #[case::same_target_kept_once(
        conditional_hook(false, 0x1006, false, 0x1006),
        ExpectedHookWalk {
            jumps: &[0x1006],
            runs_original: false,
            instructions: &[0x1000, 0x1006],
            successors: &[(0x1006, EdgeKind::Jump)],
            callees: &[],
            functions: &[0x1000, 0x1005, 0x1006],
        }
    )]
    fn walk_should_follow_hook_flow(#[case] hook: MidAsmHook, #[case] expected: ExpectedHookWalk) {
        // Arrange: each case hooks the call in this shared body.
        // ```text
        // 0x1000: E8 03 00 00 00  call 0x1008  ; hook runs before this call
        // 0x1005: C3              ret          ; fallthrough and true jump target
        // 0x1006: C3              ret          ; plain/false jump target
        // 0x1007: CC              int3         ; unreachable padding
        // 0x1008: C3              ret          ; callee, not part of this body
        //
        // Case                     Plain path / true path       False path
        // void_continues           run call                     -
        // void_returns             return, ignore jump 0x1006   -
        // void_jumps               jump 0x1006                  -
        // true_branch_jumps        jump 0x1005                  run call
        // false_branch_jumps       run call                     jump 0x1006
        // both_branches_jump       jump 0x1005                  jump 0x1006
        // true_return_beats_jump   return, ignore jump 0x1005   run call
        // false_return_beats_jump  run call                     return, ignore jump 0x1006
        // both_branches_return     return                       return
        // same_target_kept_once    jump 0x1006                  jump 0x1006
        // ```
        //
        // Sewer Note: This is a PITA to understnd. Huge ass PITA.
        //             This REALLY needs cleanup.
        let executable = executable(&[
            0xe8, 0x03, 0x00, 0x00, 0x00, // 0x1000: call 0x1008 (hook runs first)
            0xc3, // 0x1005: ret (call fallthrough and true jump target)
            0xc3, // 0x1006: ret (plain and false jump target)
            0xcc, // 0x1007: int3 (unreachable padding)
            0xc3, // 0x1008: ret (callee, not part of this body)
        ]);
        let mut config = RecompilerConfig::default();
        config
            .midasm_hooks
            .insert_or_overwrite(FunctionAddress(BASE), hook.clone());
        let decoder = Decoder::new_compatibility_mode();
        let mut walker = FunctionWalker::new(&executable, &config, &decoder);
        let mut graph = DiscoveryGraph::default();
        graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);
        // Known entries split blocks; hook jumps to them stay local.
        graph.add_function(at(5), FunctionDiscoverySource::Symbol, None);
        graph.add_function(at(6), FunctionDiscoverySource::Symbol, None);

        // Act.
        let classified = flow(&hook);
        let update = walker.walk_function(&mut graph, base()).unwrap();

        // Assert: hook jumps and whether any path runs the call.
        let jump_targets: Vec<u32> = classified
            .jumps
            .iter()
            .map(|address| address.get())
            .collect();
        assert_eq!(jump_targets, expected.jumps);
        assert_eq!(classified.runs_original, expected.runs_original);

        // Assert: reached instructions and edges from the hook site.
        let node = &graph.functions[&base()];
        assert_eq!(
            node.instructions
                .keys()
                .map(|address| address.get())
                .collect::<Vec<_>>(),
            expected.instructions
        );
        let successors = &node.block(base()).unwrap().successors;
        assert!(successors.iter().all(|edge| edge.source == base()));
        assert_eq!(
            successors
                .iter()
                .map(|edge| (edge.target.get(), edge.kind))
                .collect::<Vec<_>>(),
            expected.successors
        );

        // Assert: only the original call adds a reference, never a hook jump.
        assert!(node.references.iter().all(|reference| {
            reference.site == base() && reference.kind == ReferenceKind::DirectCall
        }));
        assert_eq!(
            node.references
                .iter()
                .map(|reference| reference.target.get())
                .collect::<Vec<_>>(),
            expected.callees
        );
        // Only the callee can be new; the graph already has both hook targets.
        assert_eq!(
            update
                .new_functions
                .iter()
                .map(|address| address.get())
                .collect::<Vec<_>>(),
            expected.callees
        );
        assert_eq!(
            graph
                .functions
                .keys()
                .map(|address| address.get())
                .collect::<Vec<_>>(),
            expected.functions
        );
        assert!(node.unresolved.is_empty());
        assert!(node.diagnostics.is_empty());
    }

    #[test]
    fn walk_should_mark_hooked_instruction_without_changing_decode_cache() {
        // Arrange:
        // ```text
        // 0x1000: B8 07 00 00 00  mov eax,7
        // 0x1005: 90              nop  ; plain hook here
        // 0x1006: C3              ret
        // ```
        let executable = executable(&[0xb8, 7, 0, 0, 0, 0x90, 0xc3]);
        let mut config = RecompilerConfig::default();
        config
            .midasm_hooks
            .insert_or_overwrite(FunctionAddress(BASE + 5), void_hook(false, 0));
        let decoder = Decoder::new_compatibility_mode();
        let mut walker = FunctionWalker::new(&executable, &config, &decoder);
        let mut graph = DiscoveryGraph::default();
        graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);

        // Act.
        walker.walk_function(&mut graph, base()).unwrap();
        let repeated = walker.walk_function(&mut graph, base()).unwrap();
        let node = &graph.functions[&base()];

        // Assert: only the body's instruction copy carries the hook's marker.
        assert!(node.instructions[&at(5)].clobbers_all);
        assert!(!walker.cache[&at(5)].as_ref().unwrap().clobbers_all);
        assert!(!repeated.changed);
    }

    /// A conditional hook with the per-branch return and jump keys.
    fn conditional_hook(
        return_on_true: bool,
        jump_on_true: u32,
        return_on_false: bool,
        jump_on_false: u32,
    ) -> MidAsmHook {
        MidAsmHook {
            name: "watch".into(),
            return_on_true,
            jump_address_on_true: jump_on_true,
            return_on_false,
            jump_address_on_false: jump_on_false,
            ..MidAsmHook::default()
        }
    }

    /// A void hook with the `return` and `jump_address` keys.
    fn void_hook(ret: bool, jump_address: u32) -> MidAsmHook {
        MidAsmHook {
            name: "watch".into(),
            ret,
            jump_address,
            ..MidAsmHook::default()
        }
    }
}
