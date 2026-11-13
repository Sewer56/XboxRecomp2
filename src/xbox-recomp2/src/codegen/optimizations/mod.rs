//! Runs the enabled optimisations on functions.

use crate::analysis::FunctionGraph;
use crate::codegen::function::model::FunctionModel;
use crate::config::{OptimizationHints, Optimizations, SpeedHacks};
use crate::executable::ImportMap;
use helpers::flag_boundaries::FlagBoundaries;
use std::io::Write;

mod eliminate_dead_flags;
mod eliminate_dead_x87_status;
pub(in crate::codegen) mod eliminate_return_slot;
pub(in crate::codegen) mod eliminate_stack_args;
mod find_ebp_frames;
mod find_lock_releases;
mod find_nonvolatile_memory;
mod find_string_direction;
pub(in crate::codegen) mod helpers;
mod idiv32_after_cdq;
mod merge_cmp_jmp;
mod mmx_locals;
mod stack_slots_in_locals;
mod x87;

/// Runs all optimisation passes.
pub(crate) fn run(
    models: &mut [FunctionModel],
    graph: &FunctionGraph,
    imports: &ImportMap,
    hints: &OptimizationHints,
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

    // After the stack passes, which change pushes and ESP offsets.
    // Before `fuse_compare_branch`, whose merged ops the stack walk can't read.
    if hacks.runs_stack_slots_in_locals() {
        stack_slots_in_locals::run(models, diag);
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

    if switches.runs_optimize_x87() {
        x87::run(
            models,
            graph,
            imports,
            &hints.x87_stack_effects,
            x87::Hacks {
                fast_trig: hacks.runs_fast_trig(),
                fast_ffree: hacks.runs_fast_x87_ffree(),
            },
            diag,
        );
    }
    if switches.runs_optimize_mmx() {
        mmx_locals::run(models, diag);
    }
    if switches.runs_eliminate_dead_x87_status() {
        eliminate_dead_x87_status::run(models);
    }

    if switches.runs_find_ebp_frames() {
        find_ebp_frames::run(models);
    }

    if switches.runs_idiv32_after_cdq() {
        for model in models.iter_mut() {
            idiv32_after_cdq::run(model);
        }
    }

    if switches.runs_find_string_direction() {
        find_string_direction::run(models, diag);
    }

    if hacks.runs_plain_lock_release() {
        for model in models.iter_mut() {
            find_lock_releases::run(model);
        }
    }

    if hacks.runs_nonvolatile_memory() || hacks.runs_nonvolatile_stack() {
        find_nonvolatile_memory::run(models, hacks);
    }
}
