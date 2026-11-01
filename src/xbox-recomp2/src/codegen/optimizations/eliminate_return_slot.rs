//! Doesn't put the return address on the stack for function calls where possible.
//!
//! See `eliminate-return-slot.md` for more info.

use super::helpers::function_stack::{FunctionStack, no_calls};
use super::helpers::known_callers::{self, direct_call_target};
use super::helpers::stack_frame::STACK_WORD_BYTES;
use super::helpers::stack_shift::StackShift;
use crate::analysis::FunctionGraph;
use crate::codegen::function::model::{Entry, FunctionModel, Op};
use std::collections::BTreeMap;
use std::io::Write;

pub(in crate::codegen) fn run(
    models: &mut [FunctionModel],
    graph: &FunctionGraph,
    diag: &mut dyn Write,
) {
    // Find callees we can rewrite, with all callers known.
    let mut candidates: BTreeMap<_, _> = models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| Some((model.address, (index, try_plan_shift(model)?))))
        .collect();
    known_callers::retain(&mut candidates, models, graph, |_, _, _| true);

    // Rewrite the callers.
    let mut rewritten = 0;
    for op in models
        .iter_mut()
        .flat_map(|model| &mut model.blocks)
        .flat_map(|block| &mut block.ops)
    {
        if let Op::Call(call) = op
            && direct_call_target(&call.guest)
                .is_some_and(|target| candidates.contains_key(&target))
        {
            call.stack_bytes -= STACK_WORD_BYTES; // <- change bytes on stack in call intrinsic
            call.calls_fastcall = true;
            rewritten += 1;
        }
    }

    // Rewrite the callees.
    for (index, shift) in candidates.values() {
        let model = &mut models[*index];
        shift.apply(model);
        model.protect_abi();
        model.fastcall.omits_return_slot = true;
        for op in model.blocks.iter_mut().flat_map(|block| &mut block.ops) {
            if let Op::Return(ret) = op {
                ret.omit_return_slot = true; // <- rewrite
            }
        }
    }

    let _ = writeln!(
        diag,
        "eliminate-return-slot: rewrote {rewritten} calls ({} callees)",
        candidates.len()
    );
}

/// Plans moving the arguments down onto the return address.
fn try_plan_shift(model: &FunctionModel) -> Option<StackShift> {
    if !model.can_change_abi() || model.entry != Entry::First {
        return None;
    }
    let stack = FunctionStack::new(model, no_calls)?;
    if stack.return_slot_bytes != STACK_WORD_BYTES {
        return None; // already done
    }
    let slot = STACK_WORD_BYTES as i32;
    StackShift::find(&stack, slot, -slot)
}
