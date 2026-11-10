//! Counts `#stack`, the number of values on the x87 stack, in every function.
//!
//! See "Counting the stack" in `x87.md`.
//!
//! WARNING. EXPERIMENTAL.
//! THIS CODE SUCKS.
//! THIS CODE SUCKS.
//! THIS CODE SUCKS.
//!
//! IT WORKS, BUT
//!
//! THIS CODE SUCKS.
//! THIS CODE SUCKS.
//! THIS CODE SUCKS.

use super::Depth;
use super::call_target::{CallTarget, CallTargets};
use crate::codegen::optimizations::helpers::model_index::ModelIndex;
use crate::codegen::optimizations::helpers::op_graph::{OpGraph, OpIndex};
use crate::config;
use core::mem;
pub(super) use entry::{CalledBy, find_who_may_call};
use function::count_function;
use std::collections::{BTreeMap, BTreeSet};
use xbox_recomp2_executable::GuestAddress32;

mod arguments;
mod entry;
mod function;
mod patterns;

/// Most rounds of "count every function" before giving up.
///
/// Shadow finishes in 5.
/// The cap is for cycles: unlikely, but possible.
const MAX_ROUNDS: usize = 50;

/// Every function's count, once no [`X87Effect`] changes.
struct SettledCounts {
    functions: Vec<FunctionAnalysis>,
    effects: Vec<X87Effect>,
}

/// What we found about the x87 stack in every function.
///
/// Each list has one item per function, in model order.
pub(super) struct X87Analysis {
    pub functions: Vec<FunctionAnalysis>,

    /// What calling each function does.
    pub effects: Vec<X87Effect>,

    /// Whether we trust each function: neither it nor anything it calls is opaque.
    pub trusted: Vec<bool>,
}

/// What we found about the x87 stack in one function.
pub(super) struct FunctionAnalysis {
    /// Starts with [`Self::args`] values, or [`Self::entry`] once step 5 finds it.
    pub count: StackCount,

    /// How many x87 arguments it takes.
    pub args: Depth,

    /// Whether it also has x87 arguments only a called function reads.
    pub hidden_args: bool,

    /// Why the function is opaque, if it is.
    pub opaque: Option<OpaqueReason>,

    /// `#stack` when it starts.
    /// [`None`] if callers disagree, or we can't tell.
    pub entry: Option<Depth>,
}

/// What calling a function does to the x87 stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct X87Effect {
    /// Values on the stack at entry: its x87 arguments.
    pub stack_at_entry: Depth,

    /// Values on the stack at exit: 1 if it returns a float in `st0`, else 0.
    pub stack_at_exit: Depth,
}

/// `#stack` throughout one function, for one starting depth.
#[derive(Debug, Default)]
pub(super) struct StackCount {
    /// `#stack` before each op. [`None`] if unreachable.
    pub before: Vec<Option<Depth>>,

    /// `#stack` after each call returns.
    pub after_call: BTreeMap<OpIndex, Depth>,

    /// Each call and tail call.
    pub calls: Vec<CallSite>,

    /// `#stack` at each exit: `ret`, tail call, or hook that returns.
    pub exits: Vec<Depth>,

    /// Each op that reads values we never loaded.
    pub missing_values: Vec<MissingValues>,

    /// Each call that takes more x87 arguments than `#stack` holds.
    pub calls_missing_values: Vec<OpIndex>,

    /// Each `ffree` we count as a pop.
    pub ffree_pops: BTreeSet<GuestAddress32>,

    /// Each `frstor` op given the `#stack` from before its `fnsave`.
    pub paired_frstors: BTreeSet<OpIndex>,

    /// Whether we guessed what a virtual call or jump returned.
    pub guessed: bool,

    /// Why the count is opaque, if it is.
    pub opaque: Option<OpaqueReason>,
}

/// One call or tail call.
#[derive(Clone, Copy, Debug)]
pub(super) struct CallSite {
    pub op: OpIndex,

    /// The guest instruction that calls.
    pub address: GuestAddress32,

    pub target: CallTarget,

    /// `#stack` when the called function starts.
    pub depth: Depth,
}

/// An op that reads values we never loaded.
#[derive(Clone, Copy, Debug)]
pub(super) struct MissingValues {
    pub op: OpIndex,

    /// How many values it lacks.
    pub missing: Depth,
}

/// Why a function is opaque: we can't count its `#stack`.
///
/// See "4. Give up on what we can't follow" in `x87.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum OpaqueReason {
    /// Two paths meet with different `#stack`.
    DepthConflict,

    /// An instruction moves the stack in a way we can't count.
    DepthUnknown,

    /// A decode failure, or a jump or call we can't follow.
    Unfollowable,

    /// Paths exit with different `#stack`, or with more than 1 value.
    ExitConflict,

    /// After a call, something reads more values than `#stack` holds.
    MissingValuesAfterCall,

    /// It passes extra values to a function with a virtual call.
    PassesExtraValues,
}

impl X87Analysis {
    /// Counts `#stack` in every function.
    /// [`None`] if the effects still change after [`MAX_ROUNDS`].
    pub(super) fn build(
        graphs: &[Option<OpGraph>],
        call_targets: &CallTargets,
        config_effects: &[Option<X87Effect>],
        called_by: &[CalledBy],
    ) -> Option<Self> {
        // Steps 1 to 3: count every function, until no `X87Effect` changes.
        let SettledCounts {
            mut functions,
            effects,
        } = count_until_settled(graphs, call_targets, config_effects)?;

        // Step 4: give up on what we can't follow.
        let callers_of = list_callers(&functions);
        let may_read_extra_values =
            find_extra_value_readers(&functions, &callers_of, config_effects);
        mark_extra_value_passers(&mut functions, &effects, &may_read_extra_values);
        let trusted = find_trusted_functions(&functions, &callers_of, config_effects);

        // Step 5: find `#stack` at entry.
        let finder = entry::EntryFinder {
            graphs,
            call_targets,
            effects: &effects,
            trusted: &trusted,
            may_read_extra_values: &may_read_extra_values,
            called_by,
        };

        entry::find_stack_at_entry(&finder, &mut functions);

        Some(Self {
            functions,
            effects,
            trusted,
        })
    }
}

impl FunctionAnalysis {
    /// What calling the function does, going by its count.
    pub(super) fn counted_effect(&self) -> X87Effect {
        let returns_float = self.count.exits.iter().any(|&exit| exit >= 1);
        X87Effect {
            stack_at_entry: self.args,
            stack_at_exit: Depth::from(returns_float),
        }
    }
}

impl X87Effect {
    /// Leaves the stack alone.
    pub(super) const NONE: Self = Self {
        stack_at_entry: 0,
        stack_at_exit: 0,
    };
}

impl From<config::X87StackEffect> for X87Effect {
    fn from(effect: config::X87StackEffect) -> Self {
        Self {
            stack_at_entry: effect.stack_at_entry,
            stack_at_exit: effect.stack_at_exit,
        }
    }
}

// ── 3. Repeat until no `X87Effect` changes ───────────────────────────────────

/// Counts every function, again and again, until no [`X87Effect`] changes.
fn count_until_settled(
    graphs: &[Option<OpGraph>],
    call_targets: &CallTargets,
    config_effects: &[Option<X87Effect>],
) -> Option<SettledCounts> {
    // Do the initial count
    let no_effects = vec![X87Effect::NONE; graphs.len()];
    let first = count_every_function(graphs, call_targets, &no_effects);
    let mut effects = Vec::with_capacity(graphs.len());
    for (function, config_effect) in first.iter().zip(config_effects) {
        effects.push(config_effect.unwrap_or(X87Effect {
            stack_at_entry: function.args,
            stack_at_exit: 0,
        }));
    }

    // Repeat, repeat, repeat. Till nyothing changes!
    for _ in 0..MAX_ROUNDS {
        let functions = count_every_function(graphs, call_targets, &effects);
        let mut next = Vec::with_capacity(graphs.len());
        for (function, config_effect) in functions.iter().zip(config_effects) {
            next.push(config_effect.unwrap_or_else(|| function.counted_effect()));
        }

        if next == effects {
            return Some(SettledCounts { functions, effects });
        }

        effects = next;
    }

    None
}

/// Counts every function with the current `effects`.
fn count_every_function(
    graphs: &[Option<OpGraph>],
    call_targets: &CallTargets,
    effects: &[X87Effect],
) -> Vec<FunctionAnalysis> {
    let mut functions = Vec::with_capacity(graphs.len());
    for graph in graphs {
        functions.push(match graph {
            Some(graph) => count_function(graph, call_targets, effects),
            None => FunctionAnalysis {
                count: StackCount::default(),
                args: 0,
                hidden_args: false,
                opaque: Some(OpaqueReason::Unfollowable),
                entry: None,
            },
        });
    }

    functions
}

/// Whether each function may read values below its x87 arguments.
///
/// That is, it makes a virtual call, or calls something that does.
fn find_extra_value_readers(
    functions: &[FunctionAnalysis],
    callers_of: &[Vec<ModelIndex>],
    config_effects: &[Option<X87Effect>],
) -> Vec<bool> {
    let mut may_read = Vec::with_capacity(functions.len());
    for function in functions {
        let calls = &function.count.calls;
        may_read.push(calls.iter().any(|call| call.target == CallTarget::Indirect));
    }

    spread_to_callers(&mut may_read, callers_of, |_| true);

    // A config effect or hidden arguments already explain the values.
    for ((may_read, function), config_effect) in
        may_read.iter_mut().zip(functions).zip(config_effects)
    {
        *may_read &= config_effect.is_none() && !function.hidden_args;
    }

    may_read
}

// ── 4. Give up on what we can't follow ───────────────────────────────────────

/// Whether we trust each function: neither it nor anything it calls is opaque.
fn find_trusted_functions(
    functions: &[FunctionAnalysis],
    callers_of: &[Vec<ModelIndex>],
    config_effects: &[Option<X87Effect>],
) -> Vec<bool> {
    // A config effect on an opaque function keeps its callers trusted.
    let mut untrusted: Vec<_> = functions.iter().map(|f| f.opaque.is_some()).collect();
    spread_to_callers(&mut untrusted, callers_of, |f| config_effects[f].is_none());

    untrusted.into_iter().map(|untrusted| !untrusted).collect()
}

/// Marks every function that calls a marked one, directly or through others.
fn spread_to_callers(
    marked: &mut [bool],
    callers_of: &[Vec<ModelIndex>],
    passes_on: impl Fn(ModelIndex) -> bool,
) {
    let mut pending: Vec<_> = (0..marked.len())
        .filter(|&f| marked[f])
        .map(ModelIndex::from)
        .collect();

    while let Some(called) = pending.pop() {
        if !passes_on(called) {
            continue;
        }

        for &caller in &callers_of[called] {
            if !mem::replace(&mut marked[caller], true) {
                pending.push(caller);
            }
        }
    }
}

/// The functions that call each function, through a call or tail call.
fn list_callers(functions: &[FunctionAnalysis]) -> Vec<Vec<ModelIndex>> {
    let mut callers_of = vec![Vec::new(); functions.len()];
    for (caller, function) in functions.iter().enumerate() {
        for call in &function.count.calls {
            if let CallTarget::Function(called) = call.target {
                callers_of[called].push(ModelIndex::from(caller));
            }
        }
    }

    callers_of
}

/// Marks opaque each function that passes extra values to one that may read them.
fn mark_extra_value_passers(
    functions: &mut [FunctionAnalysis],
    effects: &[X87Effect],
    may_read_extra_values: &[bool],
) {
    // Example: `f` takes 1 value, but reads it after a virtual call.
    //
    //   caller: fld  [ecx]    ; 0 -> 1
    //           call f        ; we think f takes 0, so 1 -> 1
    //   f:      call [eax]    ; we think it returns 1
    //           fstp [ecx]    ; pops the argument, we think it pops the return
    //
    // So f's effect is wrong, and so is the caller's count after the call.
    for caller in functions {
        if passes_extra_values(&caller.count, effects, may_read_extra_values) {
            caller.opaque = caller.opaque.or(Some(OpaqueReason::PassesExtraValues));
        }
    }
}

/// Whether `count` passes extra values to a function that may read them.
fn passes_extra_values(
    count: &StackCount,
    effects: &[X87Effect],
    may_read_extra_values: &[bool],
) -> bool {
    for call in &count.calls {
        let called = match call.target {
            CallTarget::Function(called) => called,
            CallTarget::Import | CallTarget::Indirect | CallTarget::Unknown => continue,
        };

        if call.depth > effects[called].stack_at_entry && may_read_extra_values[called] {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::super::Hacks;
    use super::*;
    use crate::codegen::optimizations::helpers::tests::{executable, layout, program};
    use crate::config::{FunctionAddress, RecompilerConfig};
    use crate::executable::ImportMap;
    use rstest::rstest;

    // ── Called functions at 0x1040 ───────────────────────────────────────────

    /// No function at 0x1040.
    const NO_CALLED_FUNCTION: &[u8] = &[];

    /// `ret`
    const RET: &[u8] = &[0xC3];

    /// `fld [ecx]; ret`
    const RETURNS_FLOAT: &[u8] = &[0xD9, 0x01, 0xC3];

    /// `fstp [ecx]; ret`
    const TAKES_ONE: &[u8] = &[0xD9, 0x19, 0xC3];

    /// `fstp [ecx]; fld [ecx]; ret`
    const TAKES_ONE_RETURNS_FLOAT: &[u8] = &[0xD9, 0x19, 0xD9, 0x01, 0xC3];

    /// `call [eax]; fstp [ecx]; ret`
    const VIRTUAL_CALL: &[u8] = &[0xFF, 0x10, 0xD9, 0x19, 0xC3];

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// What the analysis found for one function.
    #[derive(Debug, PartialEq, Eq)]
    struct Found {
        stack_at_entry: Depth,
        stack_at_exit: Depth,
        hidden_args: bool,
        opaque: Option<OpaqueReason>,
        trusted: bool,
        entry: Option<Depth>,
    }

    impl Default for Found {
        /// A trusted function that starts with an empty stack and leaves it empty.
        fn default() -> Self {
            Self {
                stack_at_entry: 0,
                stack_at_exit: 0,
                hidden_args: false,
                opaque: None,
                trusted: true,
                entry: Some(0),
            }
        }
    }

    /// Runs [`X87Analysis::build`] over `caller` at 0x1000 and `called` at 0x1040.
    ///
    /// Returns what it found for the function at `address`.
    fn analyse(
        caller: &[u8],
        called: &[u8],
        config_effect: Option<X87Effect>,
        address: u32,
    ) -> Found {
        // Build every function.
        let code = match called.is_empty() {
            true => layout(&[(0x1000, caller)]),
            false => layout(&[(0x1000, caller), (0x1040, called)]),
        };

        let (graph, models) = program(&mut executable(&code), &RecompilerConfig::default());
        let graphs: Vec<_> = models.iter().map(OpGraph::new).collect();
        let imports = ImportMap::default();
        let call_targets = CallTargets::new(&models, &imports, Hacks::ALL);
        let config_effects: Vec<_> = models
            .iter()
            .map(|model| config_effect.filter(|_| model.address == FunctionAddress(0x1040)))
            .collect();

        // Count.
        let called_by = find_who_may_call(&models, &graph);
        let analysis =
            X87Analysis::build(&graphs, &call_targets, &config_effects, &called_by).unwrap();

        // Pick out the function.
        let index = models
            .iter()
            .position(|model| model.address == FunctionAddress(address))
            .unwrap();
        let effect = analysis.effects[index];
        let function = &analysis.functions[index];
        Found {
            stack_at_entry: effect.stack_at_entry,
            stack_at_exit: effect.stack_at_exit,
            hidden_args: function.hidden_args,
            opaque: function.opaque,
            trusted: analysis.trusted[index],
            entry: function.entry,
        }
    }

    // ── Steps 1 to 4: counting ───────────────────────────────────────────────

    /// What `build` finds for the caller.
    #[rstest]
    // 1. Count the stack in each function
    #[case::returns_float(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0xC3,                      // ret              ; <- st0 holds the result
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_exit: 1,
            ..Found::default()
        },
    )]
    #[case::argument_on_one_path(
        &[
            0x85, 0xC0,                // test    eax, eax
            0x74, 0x02,                // jz      .done
            0xDC, 0xC0,                // fadd    st0, st0 ; <- reads 1 value it never loaded
                                       // .done:
            0xD9, 0x19,                // fstp    [ecx]    ; pops it on both paths
            0xC3,                      // ret
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_entry: 1,
            entry: Some(1),
            ..Found::default()
        },
    )]
    #[case::ffree_run_pops(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0xD9, 0x01,                // fld     [ecx]
            0xDD, 0xC1,                // ffree   st1      ; <- bottom of 2: a pop
            0xDD, 0xC0,                // ffree   st0      ; <- bottom of 1: a pop
            0xC3,                      // ret
        ],
        NO_CALLED_FUNCTION,
        Found::default(),
    )]
    // 2. At each call, use the called function's count
    #[case::called_function_returns_float(
        &[
            0xE8, 0x3B, 0, 0, 0,       // call    0x1040   ; leaves a float
            0xD9, 0x19,                // fstp    [ecx]    ; <- pops it
            0xC3,                      // ret
        ],
        RETURNS_FLOAT,
        Found::default(),
    )]
    #[case::virtual_read_after_push(
        &[
            0xFF, 0x10,                // call    [eax]
            0xD9, 0x01,                // fld     [ecx]    ; pushed 1
            0xDF, 0xF1,                // fcomip  st0, st1 ; <- reads 2: the call returned 1
            0xDD, 0xD8,                // fstp    st0
            0xC3,                      // ret
        ],
        NO_CALLED_FUNCTION,
        Found::default(),
    )]
    #[case::virtual_unread_on_one_path(
        &[
            0x85, 0xC0,                // test    eax, eax
            0x74, 0x04,                // jz      .error
            0xD9, 0x01,                // fld     [ecx]
            0xEB, 0x02,                // jmp     .done
                                       // .error:
            0xFF, 0x10,                // call    [eax]    ; <- nothing reads it: guess 1
                                       // .done:
            0xC3,                      // ret              ; the other path returns 1
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_exit: 1,
            ..Found::default()
        },
    )]
    #[case::argument_read_after_call(
        &[
            0xE8, 0x3B, 0, 0, 0,       // call    0x1040   ; pushes 1
            0xDE, 0xC1,                // faddp   st1, st0 ; <- reads 2: one is ours
            0xD9, 0x19,                // fstp    [ecx]
            0xC3,                      // ret
        ],
        RETURNS_FLOAT,
        Found {
            stack_at_entry: 1,
            entry: Some(1),
            ..Found::default()
        },
    )]
    #[case::hidden_argument_of_tail_call(
        &[
            0x85, 0xC0,                // test    eax, eax
            0x0F, 0x84, 0x38, 0, 0, 0, // jz      0x1040   ; <- tail call with 0 values
            0xD9, 0x01,                // fld     [ecx]
            0xE8, 0x31, 0, 0, 0,       // call    0x1040   ; makes 0x1040 a function
            0xD9, 0x19,                // fstp    [ecx]
            0xC3,                      // ret
        ],
        TAKES_ONE_RETURNS_FLOAT,
        Found {
            hidden_args: true,
            entry: None,
            ..Found::default()
        },
    )]
    // 4. Give up on what we can't follow
    #[case::paths_disagree(
        &[
            0x85, 0xC0,                // test    eax, eax
            0x74, 0x02,                // jz      .done
            0xD9, 0x01,                // fld     [ecx]    ; <- only one path pushes
                                       // .done:
            0xC3,                      // ret
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_exit: 1,
            opaque: Some(OpaqueReason::DepthConflict),
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::two_values_at_ret(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0xD9, 0x01,                // fld     [ecx]
            0xC3,                      // ret              ; <- returns 2
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_exit: 1,
            opaque: Some(OpaqueReason::ExitConflict),
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::reads_after_call_disagree(
        &[
            0xE8, 0x3B, 0, 0, 0,       // call    0x1040   ; pushes 1
            0x85, 0xC0,                // test    eax, eax
            0x74, 0x02,                // jz      .done
            0xDE, 0xC1,                // faddp   st1, st0 ; <- reads 2 on one path only
                                       // .done:
            0xD9, 0x19,                // fstp    [ecx]
            0xC3,                      // ret
        ],
        RETURNS_FLOAT,
        Found {
            opaque: Some(OpaqueReason::MissingValuesAfterCall),
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::ffree_mid_run(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0xD9, 0x01,                // fld     [ecx]
            0xDD, 0xC1,                // ffree   st1      ; <- the run stops after it
            0xD9, 0x19,                // fstp    [ecx]    ; a pop would store st1 here
            0xC3,                      // ret
        ],
        NO_CALLED_FUNCTION,
        Found {
            stack_at_exit: 1,
            opaque: Some(OpaqueReason::DepthUnknown),
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::fnsave_frstor_pair(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0x51,                      // push    ecx      ; keep the area's address
            0xDD, 0x71, 0x08,          // fnsave  [ecx+8]  ; 1 -> 0
            0xE8, 0x35, 0, 0, 0,       // call    0x1040
            0x59,                      // pop     ecx      ; the same address again
            0xDD, 0x61, 0x08,          // frstor  [ecx+8]  ; <- back to 1
            0xC3,                      // ret              ; returns it
        ],
        RET,
        Found {
            stack_at_exit: 1,
            ..Found::default()
        },
    )]
    #[case::frstor_base_changed(
        &[
            0xD9, 0x01,                // fld     [ecx]
            0xDD, 0x71, 0x08,          // fnsave  [ecx+8]
            0xE8, 0x36, 0, 0, 0,       // call    0x1040   ; may change ecx
            0xDD, 0x61, 0x08,          // frstor  [ecx+8]  ; <- ecx not kept
            0xC3,                      // ret
        ],
        RET,
        Found {
            opaque: Some(OpaqueReason::DepthUnknown),
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::called_function_may_read_extra_value(
        &[
            0xD9, 0x01,                // fld     [ecx]    ; <- the extra value
            0xE8, 0x39, 0, 0, 0,       // call    0x1040   ; <- it takes 0
            0xD9, 0x19,                // fstp    [ecx]
            0xC3,                      // ret
        ],
        VIRTUAL_CALL,
        Found {
            opaque: Some(OpaqueReason::PassesExtraValues),
            trusted: false,
            ..Found::default()
        },
    )]
    fn build_should_count_stack(
        #[case] caller: &[u8],
        #[case] called: &[u8],
        #[case] expected: Found,
    ) {
        // Act
        let actual = analyse(caller, called, None, 0x1000);

        // Assert
        assert_eq!(actual, expected);
    }

    /// A config effect replaces an opaque called function's guessed one, and
    /// keeps its callers trusted.
    #[rstest]
    #[case::guessed(
        None,
        Found {
            stack_at_exit: 1,
            trusted: false,
            ..Found::default()
        },
    )]
    #[case::config(Some(X87Effect::NONE), Found::default())]
    fn build_should_use_config_effect(
        #[case] config_effect: Option<X87Effect>,
        #[case] expected: Found,
    ) {
        // Arrange
        let caller: &[u8] = &[0xE8, 0x3B, 0, 0, 0, 0xC3]; // call 0x1040; ret
        #[rustfmt::skip]
        let called: &[u8] = &[
            0x85, 0xC0,             // test eax, eax
            0x74, 0x02,             // jz   .done
            0xD9, 0x01,             // fld  [ecx] ; only one path pushes: opaque
                                    // .done:
            0xC3,                   // ret
        ];

        // Act
        let actual = analyse(caller, called, config_effect, 0x1000);

        // Assert
        assert_eq!(actual, expected);
    }

    // ── Step 5: `#stack` at entry ────────────────────────────────────────────

    /// The called function's `#stack` at entry, from where the caller calls it.
    #[rstest]
    #[case::callers_agree(
        &[
            0xD9, 0x01,          // fld    [ecx]  ; <- left below the argument
            0xD9, 0x01,          // fld    [ecx]
            0xE8, 0x37, 0, 0, 0, // call   0x1040 ; <- starts at 2
            0xD9, 0x19,          // fstp   [ecx]
            0xC3,                // ret
        ],
        TAKES_ONE,
        Some(2), // #stack at entry
    )]
    #[case::callers_disagree(
        &[
            0xD9, 0x01,          // fld    [ecx]
            0xE8, 0x39, 0, 0, 0, // call   0x1040 ; starts at 1
            0xD9, 0x01,          // fld    [ecx]
            0xD9, 0x01,          // fld    [ecx]
            0xE8, 0x30, 0, 0, 0, // call   0x1040 ; <- starts at 2
            0xD9, 0x19,          // fstp   [ecx]
            0xC3,                // ret
        ],
        TAKES_ONE,
        None, // #stack at entry
    )]
    #[case::call_after_reset(
        &[
            0xD9, 0x01,          // fld    [ecx]
            0xDD, 0x31,          // fnsave [ecx]  ; <- 1 -> 0
            0xD9, 0x01,          // fld    [ecx]
            0xE8, 0x35, 0, 0, 0, // call   0x1040 ; starts at 1
            0xDD, 0x22,          // frstor [edx]  ; not the saved area: opaque
            0xC3,                // ret
        ],
        RET,
        Some(1), // #stack at entry
    )]
    #[case::call_without_reset(
        &[
            0xD9, 0x01,          // fld    [ecx]
            0xD9, 0x01,          // fld    [ecx]  ; <- no reset
            0xD9, 0x01,          // fld    [ecx]
            0xE8, 0x35, 0, 0, 0, // call   0x1040 ; starts at 3
            0xDD, 0x22,          // frstor [edx]  ; not the saved area: opaque
            0xC3,                // ret
        ],
        RET,
        None, // #stack at entry
    )]
    fn build_should_find_entry_depth(
        #[case] caller: &[u8],
        #[case] called: &[u8],
        #[case] expected: Option<Depth>,
    ) {
        // Act
        let actual = analyse(caller, called, None, 0x1040);

        // Assert
        assert_eq!(actual.entry, expected);
    }
}
