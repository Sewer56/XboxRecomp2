//! Runs the enabled optimisations on functions.

use crate::analysis::FunctionGraph;
use crate::codegen::function::model::FunctionModel;
use crate::config::{Optimizations, SpeedHacks};
use helpers::flag_boundaries::FlagBoundaries;
use std::io::Write;

mod eliminate_dead_flags;
pub(in crate::codegen) mod eliminate_return_slot;
pub(in crate::codegen) mod eliminate_stack_args;
mod find_ebp_frames;
pub(in crate::codegen) mod helpers;
mod idiv32_after_cdq;
mod merge_cmp_jmp;
mod x87_lowering;

/// Runs all optimisation passes.
pub(crate) fn run(
    models: &mut [FunctionModel],
    graph: &FunctionGraph,
    switches: &Optimizations,
    hacks: &SpeedHacks,
    diag: &mut dyn Write,
) {
    if switches.runs_eliminate_stack_args() {
        eliminate_stack_args::run(models, graph, switches.runs_trust_callee_returns(), diag);
    }

    // Run after stack arg elimination.
    if switches.runs_eliminate_return_slot() {
        eliminate_return_slot::run(models, graph, diag);
    }

    // eflag elimination runs last.
    //
    // For many reasons, be it rewriting instructions that emit flags, or
    // changing calls/returns.
    let merge_branches = switches.runs_fuse_compare_branch();
    let remove_dead_eflags = switches.runs_eliminate_dead_flags();
    if merge_branches || remove_dead_eflags {
        let boundaries = FlagBoundaries::build(
            models,
            graph,
            hacks.runs_clobber_eflags_for_unknown_code(),
            diag,
        );

        if merge_branches {
            merge_cmp_jmp::run(models, &boundaries);
        }

        // After merging. This is deliberate, don't reorder.
        //
        // The `merge_cmp_jmp` pass actually (often) eliminates an eflag write
        // entirely outright. Which is handy for us.
        if remove_dead_eflags {
            eliminate_dead_flags::run(models, &boundaries);
        }
    }

    if switches.runs_x87_lowering() {
        for model in models.iter_mut() {
            x87_lowering::run(model);
        }
    }

    if switches.runs_find_ebp_frames() {
        find_ebp_frames::run(models);
    }

    if switches.runs_idiv32_after_cdq() {
        for model in models.iter_mut() {
            idiv32_after_cdq::run(model);
        }
    }
}
