//! Speeds up x87 code by telling the compiler where the x87 stack is.
//!
//! See `x87.md` for how it works.
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

use super::helpers::model_index::ModelIndex;
use super::helpers::op_graph::OpGraph;
use crate::analysis::FunctionGraph;
use crate::codegen::function::model::{FunctionModel, X87Plan};
use crate::config::{self, AddressMap};
use crate::executable::ImportMap;
use call_target::CallTargets;
use stack_depth::{X87Analysis, X87Effect};
use std::io::Write;

mod call_target;
mod instruction;
mod plan;
mod stack_depth;

/// `#stack`: how many values are on the x87 stack.
type Depth = u8;

/// The speed hacks that change how we count the stack.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hacks {
    /// `fast_trig`: `fptan` and `fsincos` always push.
    pub fast_trig: bool,

    /// `fast_x87_ffree`: an `ffree` run that empties the stack counts as pops.
    pub fast_ffree: bool,
}

impl Hacks {
    /// Every hack on, as in the default config.
    #[cfg(test)]
    pub(super) const ALL: Self = Self {
        fast_trig: true,
        fast_ffree: true,
    };
}

/// Picks how to write every function's x87 code.
pub(super) fn run(
    models: &mut [FunctionModel],
    graph: &FunctionGraph,
    imports: &ImportMap,
    config_effects: &AddressMap<config::X87StackEffect>,
    hacks: Hacks,
    diag: &mut dyn Write,
) {
    // Count the stack in every function.
    let graphs: Vec<_> = models.iter().map(OpGraph::new).collect();
    let call_targets = CallTargets::new(models, imports, hacks);
    let config_effects = read_config_effects(models, config_effects, diag);
    let called_by = stack_depth::find_who_may_call(models, graph);
    let Some(analysis) = X87Analysis::build(&graphs, &call_targets, &config_effects, &called_by)
    else {
        let _ = writeln!(
            diag,
            "ERROR: x87 depth analysis didn't settle; x87 optimization skipped"
        );
        return;
    };

    warn_where_config_effect_differs(models, &config_effects, &analysis, diag);

    // Pick each function's level.
    let mut report = plan::Report::default();
    let mut plans = Vec::with_capacity(models.len());
    for (index, (model, graph)) in models.iter().zip(&graphs).enumerate() {
        let plan = match graph {
            Some(graph) => plan::plan_function(
                model,
                graph,
                ModelIndex::from(index),
                &analysis,
                &mut report,
                diag,
            ),
            None => X87Plan::default(),
        };

        plans.push(plan);
    }

    drop(graphs);
    for (model, plan) in models.iter_mut().zip(plans) {
        model.x87 = plan;
    }

    plan::write_report(&report, diag);
}

/// Each function's effect from `optimization_hints.x87_stack_effects`, if present.
fn read_config_effects(
    models: &[FunctionModel],
    config_effects: &AddressMap<config::X87StackEffect>,
    diag: &mut dyn Write,
) -> Vec<Option<X87Effect>> {
    let effects: Vec<_> = models
        .iter()
        .map(|model| {
            config_effects
                .get(model.address)
                .copied()
                .map(X87Effect::from)
        })
        .collect();

    // Warn about entries that match no function, like a typo.
    for (address, _) in config_effects.iter() {
        if !models.iter().any(|model| model.address == address) {
            let _ = writeln!(
                diag,
                "WARNING: optimization_hints.x87_stack_effects lists 0x{:X}, which isn't a function",
                address.0
            );
        }
    }

    effects
}

/// Warns where a config effect differs from what we counted.
fn warn_where_config_effect_differs(
    models: &[FunctionModel],
    config_effects: &[Option<X87Effect>],
    analysis: &X87Analysis,
    diag: &mut dyn Write,
) {
    for ((model, config_effect), function) in
        models.iter().zip(config_effects).zip(&analysis.functions)
    {
        let Some(config_effect) = config_effect else {
            continue;
        };

        let counted = function.counted_effect();
        if counted != *config_effect {
            let _ = writeln!(
                diag,
                "WARNING: optimization_hints.x87_stack_effects says 0x{:X} has stack_at_entry = {} and stack_at_exit = {}, but the analysis found {} and {}",
                model.address.0,
                config_effect.stack_at_entry,
                config_effect.stack_at_exit,
                counted.stack_at_entry,
                counted.stack_at_exit
            );
        }
    }
}
