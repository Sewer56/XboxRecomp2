//! Builds [`FlagBoundaries`] from every function's calls and returns.

use super::FlagBoundaries;
use crate::analysis::{FunctionDiscoverySource, FunctionGraph};
use crate::codegen::function::model::{ALL_ARITHMETIC_FLAGS, Edge, FunctionAbi, FunctionModel, Op};
use crate::codegen::optimizations::helpers::flag_summary::FlagSummary;
use crate::codegen::optimizations::helpers::known_callers::{direct_call_target, is_code_source};
use crate::codegen::optimizations::helpers::model_index::ModelIndex;
use crate::codegen::optimizations::helpers::needed_flags::graph::{FlagGraph, Solution};
use crate::codegen::optimizations::helpers::workqueue::WorkQueue;
use crate::decode::CpuFlag;
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// What we know about one function.
struct Function<'a> {
    /// [`None`] if the function has no entry instruction.
    graph: Option<FlagGraph<'a>>,

    /// The direct calls and tail jumps it makes.
    callees: Vec<CallSite>,

    /// The functions that call or jump to it.
    callers: Vec<ModelIndex>,

    /// Whether its `ret` can keep fewer flags.
    can_drop_return_flags: bool,
}

/// A direct call or tail jump to a known function.
#[derive(Clone, Copy)]
struct CallSite {
    /// Called function's address.
    target: GuestAddress32,

    /// Called function's position in `models`.
    callee: ModelIndex,

    /// Address of the call or jump.
    site: GuestAddress32,

    /// Operation index of the call, or [`None`] for a tail jump.
    call_at: Option<usize>,
}

/// Builds the boundaries for every function in `models`.
pub(super) fn build(
    models: &[FunctionModel],
    function_graph: &FunctionGraph,
    clobber_eflags_for_unknown_code: bool,
) -> FlagBoundaries {
    // Map each function's address to its position in `models`.
    let address_to_index: BTreeMap<GuestAddress32, ModelIndex> = models
        .iter()
        .enumerate()
        .map(|(position, model)| (model.address.0.into(), ModelIndex::from(position)))
        .collect();

    // Find who calls who, and which `ret`s can keep fewer flags.
    let functions = find_functions(
        models,
        function_graph,
        &address_to_index,
        clobber_eflags_for_unknown_code,
    );

    // Start by assuming every function clobbers flags.
    let mut boundaries = FlagBoundaries {
        address_to_index,
        summaries: vec![FlagSummary::CLOBBER; models.len()],
        at_return: Vec::new(), // <- `find_flags_at_return` fills it
        unknown: if clobber_eflags_for_unknown_code {
            FlagSummary::CLOBBER
        } else {
            FlagSummary::BARRIER
        },
    };

    // Work out which flags each function needs at entry, then at return.
    find_flags_at_entry(models, &functions, &mut boundaries);
    boundaries.at_return = find_flags_at_return(&functions, &boundaries);
    boundaries
}

/// Finds which flags each function reads and which it leaves unchanged.
///
/// Fills `boundaries.summaries`.
fn find_flags_at_entry(
    models: &[FunctionModel],
    functions: &[Function],
    boundaries: &mut FlagBoundaries,
) {
    let mut solution = Solution::default();
    let mut function_queue = WorkQueue::<ModelIndex>::full(models.len());

    // Now go through all items.
    //
    // Basically, we work through the queue, and if a summary changed as a result
    // of changes elsewhere, we re-process.
    while let Some(function) = function_queue.pop() {
        let summary = flags_at_entry(
            &models[function],
            &functions[function],
            boundaries,
            &mut solution,
        );

        // Summary unchanged since last call, no need to requeue.
        if summary == boundaries.summaries[function] {
            continue;
        }

        // The summary changed, so we need to re-queue callers.
        boundaries.summaries[function] = summary;
        for &caller in &functions[function].callers {
            function_queue.push(caller);
        }
    }
}

/// Finds which flags callers read after each function returns.
///
/// Needs `boundaries.summaries` from [`find_flags_at_entry`].
fn find_flags_at_return(functions: &[Function], boundaries: &FlagBoundaries) -> Vec<CpuFlag> {
    // Start each `ret` with no flags if it can drop them, or every flag if not.
    let mut at_return = Vec::with_capacity(functions.len());
    for function in functions {
        if function.can_drop_return_flags {
            at_return.push(CpuFlag::empty());
        } else {
            at_return.push(ALL_ARITHMETIC_FLAGS);
        }
    }

    // Then add the flags each caller reads after its calls.
    //
    //   A:
    //      call B
    //      jz done   ; <- reads ZF, so B's `ret` must keep ZF
    let mut solution = Solution::default();
    let mut queue = WorkQueue::<ModelIndex>::full(functions.len());
    while let Some(caller) = queue.pop() {
        // Find which flags the caller reads after each of its calls.
        let walked = match &functions[caller].graph {
            Some(graph) => {
                let flag_summary = |target| boundaries.flag_summary(target);
                graph.walk(at_return[caller], flag_summary, &mut solution);
                true
            }
            None => false,
        };

        for call in &functions[caller].callees {
            let needed = match (walked, call.call_at) {
                // We couldn't walk the caller, assume it reads all.
                (false, _) => ALL_ARITHMETIC_FLAGS,

                // A call: whatever the caller reads right after it.
                (true, Some(position)) => solution.live_after[position],

                // A tail jump: the callee returns to whoever called the caller.
                (true, None) => at_return[caller],
            };

            // Add them to the callee's `ret`.
            let callee = call.callee;
            let grown = at_return[callee] | needed;
            if grown == at_return[callee] {
                continue;
            }

            // The callee's `ret` needs more flags, so look at it again.
            at_return[callee] = grown;
            queue.push(callee);
        }
    }

    at_return
}

/// Finds each function's calls, callers and whether its `ret` can drop flags.
fn find_functions<'a>(
    models: &'a [FunctionModel],
    function_graph: &FunctionGraph,
    address_to_index: &BTreeMap<GuestAddress32, ModelIndex>,
    clobber_eflags_for_unknown_code: bool,
) -> Vec<Function<'a>> {
    // Now construct all the function structs.
    let mut functions: Vec<_> = models
        .iter()
        .map(|model| {
            let graph = FlagGraph::new(model);
            let callees = call_sites(model, graph.as_ref(), address_to_index); // and who the function calls!
            Function {
                graph,
                callees,
                callers: Vec::new(),
                can_drop_return_flags: false,
            }
        })
        .collect();

    // Now we which functions call others.
    // But we need the reverse lookup.
    //
    // So let's construct it with our existing data.
    //
    // From callers -> callees
    // To   callees -> callers
    for caller in 0..functions.len() {
        for position in 0..functions[caller].callees.len() {
            let callee = functions[caller].callees[position].callee;
            functions[callee].callers.push(ModelIndex::from(caller));
        }
    }

    // Now sort + dedupe.
    for function in &mut functions {
        function.callers.sort_unstable();
        function.callers.dedup();
    }

    // Pick which `ret`s can keep fewer flags.
    for (position, model) in models.iter().enumerate() {
        let entry = model.address.0.into();
        functions[position].can_drop_return_flags = clobber_eflags_for_unknown_code
            || has_only_known_callers(entry, function_graph, address_to_index, &functions);
    }

    functions
}

/// Finds `model`'s calls and jumps into known functions.
///
/// - Calls record their operation index; jumps don't.
/// - Skips indirect calls and targets not in `address_to_index`.
/// - Without a `graph`, finds only calls.
fn call_sites(
    model: &FunctionModel,
    graph: Option<&FlagGraph>,
    address_to_index: &BTreeMap<GuestAddress32, ModelIndex>,
) -> Vec<CallSite> {
    let mut found = Vec::new();
    let flow = graph.map(FlagGraph::flow);
    let ops = model.blocks.iter().flat_map(|block| &block.ops);

    // Iterate each instruction.
    for (position, op) in ops.enumerate() {
        let site = match op {
            Op::Guest(guest) => guest.address,
            Op::Call(call) => call.guest.address,
            Op::Return(ret) => ret.guest.address,
            Op::MergedCmpJmp(_) => {
                unreachable!(
                    "Did you do a fucky-wucky with the code?
                    This should run before MergedCmpJmp optimisation ❌.
                    Nyot after.
                    
                    WhAt DiD YoU Do?"
                )
            }
            Op::Failed { address, .. } => *address,
        };

        // Direct calls.
        if let Op::Call(call) = op
            && let Some(target) = direct_call_target(&call.guest)
        {
            let target = target.0.into();
            if let Some(&callee) = address_to_index.get(&target) {
                found.push(CallSite {
                    target,
                    callee,
                    site: call.guest.address,
                    call_at: Some(position),
                });
            }
        }

        // Direct jumps out of the function.
        let Some(flow) = flow else { continue };
        for edge in flow.edges(position) {
            if let Edge::Function(target) = *edge
                && let Some(&callee) = address_to_index.get(&target)
            {
                found.push(CallSite {
                    target,
                    callee,
                    site,
                    call_at: None,
                });
            }
        }
    }

    // On a misc note, it feels a bit weird to have to compute this stuff
    // after computing it during analysis. But eh- optimisations can in theory
    // remove calls. In lieu of time constraints I'll allow it.

    found
}

/// Finds which flags one function reads and which it leaves unchanged.
///
/// Uses the current summaries of the functions it calls.
fn flags_at_entry(
    model: &FunctionModel,
    function: &Function,
    boundaries: &FlagBoundaries,
    solution: &mut Solution,
) -> FlagSummary {
    if model.abi == FunctionAbi::Replaced {
        // Host code runs instead, so treat it as code we can't see.
        return boundaries.unknown;
    }
    let graph = match &function.graph {
        Some(graph) => graph,
        None => return FlagSummary::BARRIER,
    };
    let flag_summary = |target| boundaries.flag_summary(target);

    // With no flags needed after `ret`, the entry needs only what it reads.
    graph.walk(CpuFlag::empty(), flag_summary, solution);
    let reads = graph.entry(solution);

    // With every flag needed after `ret`, it also needs what it keeps.
    graph.walk(ALL_ARITHMETIC_FLAGS, flag_summary, solution);
    FlagSummary {
        reads,
        keeps: graph.entry(solution),
    }
}

/// Whether only direct calls and tail jumps in our models can reach `function`.
///
/// # Remarks
///
/// - `false` with no callers.
/// - `false` if reachable another way, e.g. a symbol or possible pointer.
/// - `true` otherwise
///
/// Basically, excludes ways where we don't know how a function is reached.
/// Because then we can't analyse the callers.
///
/// In practice it means we only leave functions reached via `call`
/// & `tail jumps` during analysis.
fn has_only_known_callers(
    function: GuestAddress32,
    function_graph: &FunctionGraph,
    address_to_index: &BTreeMap<GuestAddress32, ModelIndex>,
    functions: &[Function],
) -> bool {
    // Need at least one caller.
    let incoming = function_graph.incoming(function);
    if incoming.is_empty() {
        return false;
    }

    // Every way discovery found it must be code we walked.
    let sources = match function_graph.discovery_sources(function) {
        Some(sources) => sources,
        None => return false,
    };

    for source in sources {
        if matches!(source, FunctionDiscoverySource::Manual) {
            continue; // or the config
        }

        if !is_code_source(source, incoming, true) {
            return false;
        }
    }

    // Every caller must be one of the call sites we recorded.
    for reference in incoming {
        let caller = match address_to_index.get(&reference.source) {
            Some(&caller) => caller,
            None => return false,
        };

        let recorded = functions[caller]
            .callees
            .iter()
            .any(|call| call.site == reference.site && call.target == function);

        if !recorded {
            return false;
        }
    }

    true
}
