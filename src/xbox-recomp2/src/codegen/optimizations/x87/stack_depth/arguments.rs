//! Finds how many x87 arguments a function takes.
//!
//! See "Rare functions take arguments on the x87 stack" in `x87.md`.

use super::super::Depth;
use super::super::call_target::{CallTarget, CallTargets};
use super::function::{StackCounter, effect_of_calling, exits_conflict};
use super::{OpaqueReason, StackCount, X87Effect};
use crate::codegen::function::model::Op;
use crate::codegen::optimizations::helpers::op_graph::{OpGraph, OpIndex};
use core::mem;

/// Most times we raise one function's x87 argument count.
const MAX_ARGUMENT_ROUNDS: usize = 9;

/// How many x87 arguments a function takes, and its count starting with them.
pub(super) struct X87Arguments {
    pub args: Depth,
    pub count: StackCount,
}

/// Ops reachable from entry without passing a call that moves `#stack`.
pub(super) fn find_ops_before_any_call(
    graph: &OpGraph,
    call_targets: &CallTargets,
    effects: &[X87Effect],
) -> Vec<bool> {
    let mut seen = vec![false; graph.len()];
    let mut pending = vec![graph.entry];
    while let Some(op) = pending.pop() {
        let op = op as OpIndex;
        if mem::replace(&mut seen[op], true) {
            continue;
        }

        if let Op::Call(call) = graph.ops[op] {
            let target = call_targets.find_call_target(call);
            let leaves_stack_alone = target != CallTarget::Indirect
                && effect_of_calling(target, effects) == Some(X87Effect::NONE);
            if !leaves_stack_alone {
                continue;
            }
        }

        pending.extend(graph.local_successors(op));
    }

    seen
}

/// Finds how many x87 arguments the function takes.
pub(super) fn find_x87_arguments(counter: &StackCounter, before_calls: &[bool]) -> X87Arguments {
    // Example: `_CIpow` reads its arguments on one path but not another.
    //
    //   0x1000: fnstcw [esp]   ; no read
    //   0x1004: jz 0x100A
    //   0x1006: fadd st0, st1  ; reads 2 values it never loaded
    //   0x100A: ret
    //
    // Raising `#stack` on that path alone gives two exit depths.
    // So we raise it at entry for the whole function, and count again.
    let mut args: Depth = 0;
    let mut count = counter.count_with_arguments(args);
    for _ in 0..MAX_ARGUMENT_ROUNDS {
        let mut missing = 0;
        for read in &count.missing_values {
            if before_calls[read.op] {
                missing = missing.max(read.missing);
            }
        }

        if missing == 0 {
            break;
        }

        args = args.saturating_add(missing);
        count = counter.count_with_arguments(args);
    }

    // Reads after a call can be arguments too.
    match try_more_arguments(counter, &count, args) {
        Some(raised) => raised,
        None => X87Arguments { args, count },
    }
}

/// Tries more x87 arguments when the leftover reads come after calls.
fn try_more_arguments(
    counter: &StackCounter,
    count: &StackCount,
    args: Depth,
) -> Option<X87Arguments> {
    // Example: the `_pow` core loads its own value, then reads the argument.
    //
    //   call 0x3824F9     ; pushes 1
    //   fyl2x             ; reads 2: 1 short, after a call
    let missing = count.missing_values.iter().map(|read| read.missing).max()?;

    // Paths that already disagree won't agree on any count.
    if !matches!(count.opaque, None | Some(OpaqueReason::DepthUnknown)) {
        return None;
    }

    let raised = args.checked_add(missing)?;
    let retry = counter.count_with_arguments(raised);
    let explained = retry.opaque.is_none()
        && retry.missing_values.is_empty()
        && retry.calls_missing_values.is_empty()
        && !exits_conflict(&retry);

    explained.then_some(X87Arguments {
        args: raised,
        count: retry,
    })
}
