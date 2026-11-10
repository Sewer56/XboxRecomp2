//! Moves MMX registers into a local `uint64_t mmx[8]` where it's safe.
//!
//! See `mmx-locals.md` for more info.

use super::helpers::instruction::effects::mmx::{MmxEffect, MmxRegisters};
use super::helpers::instruction::fpu::is_mmx;
use super::helpers::op_graph::{OpGraph, OpIndex};
use crate::codegen::function::model::{FunctionModel, Op};
use std::io::Write;

pub(super) fn run(models: &mut [FunctionModel], diag: &mut dyn Write) {
    let mut local = 0usize;
    let mut kept = 0usize;
    for model in models {
        // Most functions have no MMX code, skip.
        if !uses_mmx(model) {
            continue;
        }

        // Use a local array if no other code sees the registers.
        let local_mmx = match OpGraph::new(model) {
            Some(graph) => can_localise(&graph),
            None => false,
        };

        model.local_mmx = local_mmx;
        if local_mmx {
            local += 1;
        } else {
            kept += 1;
        }
    }

    let _ = writeln!(
        diag,
        "optimize-mmx: {local} functions with local MMX registers ({kept} kept)"
    );
}

/// Whether any instruction in `model` is MMX.
fn uses_mmx(model: &FunctionModel) -> bool {
    let ops = model.blocks.iter().flat_map(|block| &block.ops);
    ops.flat_map(Op::guests).any(|guest| is_mmx(&guest.insn))
}

/// Whether the function's `mm` registers can move into a local array.
///
/// # Remarks
///
/// We assume we can localise when:
/// - `mm` register isn't a param (to this func)
/// - called functions don't need it.
/// - no mid-assembly hooks (overkillllll)
fn can_localise(graph: &OpGraph) -> bool {
    // Sanity Check! Sanity Check!
    // Any code we can't follow could do anything. Bail out.
    if graph.ops.iter().any(|op| matches!(op, Op::Failed { .. })) {
        return false;
    }

    // Find which registers each op needs from earlier code.
    let needed_before = needed_before(graph);

    if !needed_before[graph.entry as usize].is_empty() {
        return false;
    }

    for op in 0..graph.len() {
        let has_hook = graph.hook(op).is_some();
        let is_call = matches!(graph.ops[op], Op::Call(_));

        // Bail out if there's a midasm hook.
        // One day we'll sync- not today.
        if has_hook && !needed_before[op].is_empty() {
            return false;
        }

        if !is_call && !has_hook {
            continue;
        }

        for next in graph.local_successors(op) {
            if !needed_before[next as usize].is_empty() {
                return false;
            }
        }
    }
    true
}

/// Which `mm` registers each op needs from earlier code.
///
/// Return value is vector, indexed by op, stating which registers need written
/// before.
fn needed_before(graph: &OpGraph) -> Vec<MmxRegisters> {
    // Example: `mm1` is needed before the `movq`, `mm0` isn't.
    //
    //   movq mm0, mm1    ; reads mm1, overwrites mm0
    //   movq [edx], mm0  ; reads mm0
    //   emms             ; ends MMX use
    //
    // This is kind of the same 'logic' as in eflags-elimination.
    // A read of the 'skip unused flags' section would be good
    // in the wiki. I'm in a bit of a hurry, soz.

    let effects: Vec<MmxEffect> = (0..graph.len()).map(|op| op_effect(graph, op)).collect();

    graph.sweep_backward(MmxRegisters::empty(), |op, needed_before| {
        let mut needed_after = MmxRegisters::empty();

        for next in graph.local_successors(op) {
            needed_after |= needed_before[next as usize];
        }

        effects[op].needed_before(needed_after)
    })
}

/// What op `op` does to the `mm` registers.
fn op_effect(graph: &OpGraph, op: OpIndex) -> MmxEffect {
    let mut effect = MmxEffect::none();

    for guest in graph.ops[op].guests() {
        effect = effect.then(MmxEffect::of(&guest.insn));
    }

    effect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::{executable, layout, model_at, program};
    use crate::config::RecompilerConfig;
    use rstest::rstest;

    /// Registers that cross a call would be lost in a local array.
    #[rstest]
    #[case::set_and_used_locally(
        &[
            0x0F, 0x6F, 0x01,       // movq mm0, [ecx]
            0x0F, 0x7F, 0x02,       // movq [edx], mm0
            0x0F, 0x77,             // emms
            0xC3,                   // ret
        ],
        true,
    )]
    #[case::live_across_call(
        &[
            0x0F, 0x6F, 0x01,       // movq mm0, [ecx]
            0xE8, 0x38, 0, 0, 0,    // call 0x1040     ; <- mm0 still live
            0x0F, 0x7F, 0x02,       // movq [edx], mm0
            0x0F, 0x77,             // emms
            0xC3,                   // ret
        ],
        false,
    )]
    #[case::read_from_caller(
        &[
            0x0F, 0x7F, 0x02,       // movq [edx], mm0 ; <- reads the caller's mm0
            0x0F, 0x77,             // emms
            0xC3,                   // ret
        ],
        false,
    )]
    fn can_localise_should_refuse_registers_others_see(
        #[case] code: &[u8],
        #[case] expected: bool,
    ) {
        // Arrange
        let code = layout(&[(0x1000, code), (0x1040, &[0xC3])]);
        let (_, models) = program(&mut executable(&code), &RecompilerConfig::default());
        let graph = OpGraph::new(model_at(&models, 0x1000)).unwrap();

        // Act
        let actual = can_localise(&graph);

        // Assert
        assert_eq!(actual, expected);
    }
}
