//! Picks how to write each function's x87 code, and places its hints and syncs.
//!
//! See steps 6 and 7 in `x87.md`.

use super::Depth;
use super::instruction::blocks_local_copy;
use super::stack_depth::{FunctionAnalysis, OpaqueReason, StackCount, X87Analysis, X87Effect};
use crate::codegen::function::model::{
    FunctionAbi, FunctionModel, HookSync, Op, X87Level, X87Plan, X87Sync,
};
use crate::codegen::optimizations::helpers::instruction::fpu::{is_mmx, is_x87};
use crate::codegen::optimizations::helpers::model_index::ModelIndex;
use crate::codegen::optimizations::helpers::op_graph::{Edge, OpGraph, OpIndex};
use std::collections::BTreeMap;
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

/// How one op uses the local copy.
#[derive(Clone, Copy, Default)]
struct CopyUse {
    /// Has x87 code.
    uses_copy: bool,

    /// Leaves the function, so it syncs to `ctx.fpu` first.
    syncs_to_ctx: bool,

    /// Leaves on every path:
    ///
    /// - a call
    /// - a return
    /// - a plain tail call
    always_leaves: bool,

    /// Only leaves sometimes, e.g. a `jz` to another function.
    may_leave: bool,
}

/// Counts for the build log.
#[derive(Default)]
pub(super) struct Report {
    hinted: usize,
    local_copy: usize,
    skipped: BTreeMap<Skipped, usize>,
    syncs: SyncCounts,
}

/// Why a function with x87 code doesn't get the local copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Skipped {
    Opaque(OpaqueReason),
    CallsUntrustedFunction,
    UnknownEntry,
    Mmx,
    StateWide,
}

/// How many syncs of each kind we placed.
#[derive(Clone, Copy, Debug, Default)]
struct SyncCounts {
    local_to_ctx: usize,
    ctx_to_local: usize,
    hook_syncs: usize,
}

// ── 6. Pick a level ──────────────────────────────────────────────────────────

/// Picks how to write one function, and places its hints and syncs.
pub(super) fn plan_function(
    model: &FunctionModel,
    graph: &OpGraph,
    index: ModelIndex,
    analysis: &X87Analysis,
    report: &mut Report,
    diag: &mut dyn Write,
) -> X87Plan {
    // Find what the function uses.
    let function = &analysis.functions[index];
    let (mut x87, mut mmx, mut state_wide) = (false, false, false);
    for guest in graph.ops.iter().copied().flat_map(Op::guests) {
        x87 |= is_x87(&guest.insn);
        mmx |= is_mmx(&guest.insn);
        let pops = function.count.ffree_pops.contains(&guest.address);
        state_wide |= blocks_local_copy(guest.insn.mnemonic()) && !pops;
    }

    // Host replacements must move the stack the same way.
    let effect = analysis.effects[index];
    if model.abi == FunctionAbi::Replaced && effect != X87Effect::NONE {
        let _ = writeln!(
            diag,
            "WARNING: Host-replaced function 0x{:X} has {} x87 values at entry and {} at exit; keep that in the replacement",
            model.address.0, effect.stack_at_entry, effect.stack_at_exit
        );
    }

    // MMX shares the x87 registers. Skip.
    if mmx {
        if x87 {
            *report.skipped.entry(Skipped::Mmx).or_default() += 1;
        }

        return X87Plan::default();
    }

    // No x87 code, and no call moves `#stack`. Skip.
    let changes_depth = function
        .count
        .after_call
        .iter()
        .any(|(&op, &after)| function.count.before[op] != Some(after));
    if !x87 && !changes_depth {
        return X87Plan::default();
    }

    // Hints need `#stack` known everywhere.
    if let Some(reason) = reason_to_skip(function, index, analysis) {
        *report.skipped.entry(reason).or_default() += 1;
        return X87Plan::default();
    }

    let entry = function.entry.unwrap_or_default();

    // Local copy, unless the function changes state the copy doesn't carry.
    let reason = if state_wide {
        Some(Skipped::StateWide)
    } else if !x87 {
        None // only calls: nothing to copy
    } else {
        return plan_local_copy(graph, function, entry, report);
    };

    if let Some(reason) = reason {
        *report.skipped.entry(reason).or_default() += 1;
    }

    report.hinted += 1;
    plan_hints(graph, function, entry)
}

/// Writes the counts to the build log.
pub(super) fn write_report(report: &Report, diag: &mut dyn Write) {
    let _ = writeln!(
        diag,
        "optimize-x87: {} functions hinted, {} with a local copy",
        report.hinted, report.local_copy
    );
    let _ = writeln!(
        diag,
        "optimize-x87: {} syncs to ctx.fpu, {} reloads, {} hook syncs",
        report.syncs.local_to_ctx, report.syncs.ctx_to_local, report.syncs.hook_syncs
    );
    for (reason, count) in &report.skipped {
        let _ = writeln!(diag, "optimize-x87: skipped {count}: {reason:?}");
    }
}

/// Why function `index` gets no hints, if it doesn't.
fn reason_to_skip(
    function: &FunctionAnalysis,
    index: ModelIndex,
    analysis: &X87Analysis,
) -> Option<Skipped> {
    if let Some(opaque) = function.opaque {
        return Some(Skipped::Opaque(opaque));
    }

    if !analysis.trusted[index] {
        return Some(Skipped::CallsUntrustedFunction);
    }

    if function.entry.is_none() {
        return Some(Skipped::UnknownEntry);
    }

    None
}

/// The plan for a local copy that starts with `entry` values.
fn plan_local_copy(
    graph: &OpGraph,
    function: &FunctionAnalysis,
    entry: Depth,
    report: &mut Report,
) -> X87Plan {
    report.hinted += 1;
    report.local_copy += 1;
    let mut instructions = place_syncs(graph, &function.count, &mut report.syncs);
    mark_ffree_pops(&function.count, &mut instructions);
    X87Plan {
        level: X87Level::LocalCopy,
        entry,
        instructions,
    }
}

/// The plan for hints on `ctx.fpu`: at entry, labels and after calls.
fn plan_hints(graph: &OpGraph, function: &FunctionAnalysis, entry: Depth) -> X87Plan {
    let mut instructions = BTreeMap::<_, X87Sync>::new();
    for op in 0..graph.len() {
        let Some(depth) = function.count.before[op] else {
            continue;
        };

        let address = graph.ops[op].address();
        if graph.labelled[op] {
            instructions.entry(address).or_default().assume = Some(depth);
        }

        if let Some(&after) = function.count.after_call.get(&op) {
            instructions.entry(address).or_default().ctx_to_local = Some(after);
        }
    }

    mark_ffree_pops(&function.count, &mut instructions);
    X87Plan {
        level: X87Level::Hints,
        entry,
        instructions,
    }
}

// ── 7. Place the syncs ───────────────────────────────────────────────────────

/// Places the syncs for a function with a local copy, by guest address.
fn place_syncs(
    graph: &OpGraph,
    count: &StackCount,
    counts: &mut SyncCounts,
) -> BTreeMap<GuestAddress32, X87Sync> {
    let uses: Vec<_> = (0..graph.len()).map(|op| copy_use(graph, op)).collect();
    let dirty = copy_dirty_before(graph, &uses);
    let needed = copy_needed_before(graph, &uses, &dirty);

    let mut syncs = BTreeMap::<GuestAddress32, X87Sync>::new();
    for op in 0..graph.len() {
        let Some(depth) = count.before[op] else {
            continue; // unreachable op
        };

        let address = graph.ops[op].address();
        let sync = syncs.entry(address).or_default();

        // Labels: tell clang the depth, if the copy gets used after.
        if graph.labelled[op] && needed[op] {
            sync.assume = Some(depth);
        }

        // A hook works on `ctx.fpu`, so sync around it.
        if graph.hook(op).is_some() {
            sync.hook = Some(HookSync {
                depth,
                local_to_ctx: dirty[op],
            });
            counts.hook_syncs += 1;
        }

        // Sync to `ctx.fpu` before leaving, if we changed anything.
        let still_dirty = dirty[op] && graph.hook(op).is_none();
        if uses[op].syncs_to_ctx && (still_dirty || uses[op].uses_copy) {
            sync_to_ctx_at(graph, count, op, depth, &mut syncs);
            counts.local_to_ctx += 1;
        }

        // Back from a call. Reload if anything after needs the copy.
        if let Op::Call(call) = graph.ops[op]
            && let Some(&after) = count.after_call.get(&op)
            && graph
                .local_successors(op)
                .any(|next| needed[next as OpIndex])
        {
            syncs.entry(call.guest.address).or_default().ctx_to_local = Some(after);
            counts.ctx_to_local += 1;
        }
    }

    syncs.retain(|_, sync| *sync != X87Sync::default());
    syncs
}

/// Tells the writer which `ffree`s to write as pops.
fn mark_ffree_pops(count: &StackCount, instructions: &mut BTreeMap<GuestAddress32, X87Sync>) {
    for &address in &count.ffree_pops {
        instructions.entry(address).or_default().ffree_pops = true;
    }
}

/// How op `op` uses the copy.
fn copy_use(graph: &OpGraph, op: OpIndex) -> CopyUse {
    let mut uses_copy = false;
    let mut tail_jump = false;
    graph.for_each_instruction(op, |instruction| {
        uses_copy |= is_x87(&instruction.guest.insn);
        tail_jump |= matches!(instruction.jump, Some(Edge::Function(_) | Edge::Indirect));
    });

    let edges = graph.end_edges(op);
    let leaves = edges
        .iter()
        .any(|edge| matches!(edge, Edge::Function(_) | Edge::Indirect));

    let local = edges.iter().any(|edge| matches!(edge, Edge::Op(_)));
    match graph.ops[op] {
        Op::Call(_) | Op::Return(_) => CopyUse {
            uses_copy,
            syncs_to_ctx: true,
            always_leaves: true,
            may_leave: false,
        },
        _ => CopyUse {
            uses_copy,
            syncs_to_ctx: leaves || tail_jump,
            always_leaves: leaves && !local && !tail_jump,
            may_leave: (leaves && local) || tail_jump,
        },
    }
}

/// Whether the copy may hold changes `ctx.fpu` lacks, at the start of each op.
fn copy_dirty_before(graph: &OpGraph, uses: &[CopyUse]) -> Vec<bool> {
    let mut dirty = vec![false; graph.len()];
    let mut seen = vec![false; graph.len()];
    let mut pending = vec![(graph.entry, false)];
    while let Some((op, incoming)) = pending.pop() {
        let op = op as OpIndex;
        if seen[op] && (dirty[op] || !incoming) {
            continue;
        }

        seen[op] = true;
        dirty[op] |= incoming;

        // A hook syncs before the op's own code.
        let still_dirty = dirty[op] && graph.hook(op).is_none();
        let usage = uses[op];
        let touched = still_dirty || usage.uses_copy;
        let dirty_after = if usage.may_leave {
            touched // synced only on the path that leaves
        } else if usage.syncs_to_ctx {
            false
        } else {
            touched
        };

        // Hook jumps leave right after the hook's sync.
        if let Some(hook) = graph.hook(op) {
            pending.extend(hook.jumps.iter().map(|&target| (target, false)));
        }

        let targets = graph.op_targets(op).iter();
        pending.extend(targets.map(|&target| (target, dirty_after)));
    }

    dirty
}

/// Whether each op needs an up to date copy when it starts.
///
/// It does if x87 code runs later, or if we later sync to `ctx.fpu`.
/// Syncing a stale copy would undo what a call just did.
fn copy_needed_before(graph: &OpGraph, uses: &[CopyUse], dirty: &[bool]) -> Vec<bool> {
    graph.sweep_backward(false, |op, needed| {
        let usage = uses[op];
        let hooked = graph.hook(op).is_some();
        let syncs_dirty = (usage.syncs_to_ctx || hooked) && dirty[op];
        let mut next_ops = graph.local_successors(op);
        let flows_on = !usage.always_leaves && next_ops.any(|next| needed[next as OpIndex]);
        usage.uses_copy || syncs_dirty || flows_on
    })
}

/// Places the sync to `ctx.fpu` for op `op`.
///
/// For a merged compare, it goes on the jump that tail-calls.
fn sync_to_ctx_at(
    graph: &OpGraph,
    count: &StackCount,
    op: OpIndex,
    depth: Depth,
    syncs: &mut BTreeMap<GuestAddress32, X87Sync>,
) {
    // `#stack` at the point it leaves.
    let leaving_depth = |address| {
        count
            .calls
            .iter()
            .find(|call| call.address == address)
            .map_or(depth, |call| call.depth)
    };

    match graph.ops[op] {
        Op::MergedCmpJmp(_) => {
            graph.for_each_instruction(op, |instruction| {
                if matches!(instruction.jump, Some(Edge::Function(_) | Edge::Indirect)) {
                    let address = instruction.guest.address;
                    syncs.entry(address).or_default().local_to_ctx = Some(leaving_depth(address));
                }
            });
        }
        Op::Return(ret) => {
            syncs.entry(ret.guest.address).or_default().local_to_ctx = Some(depth);
        }
        _ => {
            let address = graph.ops[op].address();
            syncs.entry(address).or_default().local_to_ctx = Some(leaving_depth(address));
        }
    }
}
