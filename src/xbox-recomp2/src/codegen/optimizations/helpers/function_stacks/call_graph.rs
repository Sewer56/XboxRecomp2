//! Orders functions so we walk each one after the functions it calls.
//!
//! - [`callees_first`]: get that order.

use super::super::known_callers::direct_call_target;
use super::super::model_index::ModelIndex;
use crate::codegen::function::model::{FunctionModel, Op};
use ahash::{HashMap, HashMapExt};

/// How far [`add`] has got with one function.
#[derive(Clone, Copy)]
enum Progress {
    /// Not looked at yet.
    NotSeen,

    /// Adding its callees. Reaching it again means we went round a call loop.
    AddingCallees,

    /// In the order.
    Added,

    /// Left out, because it reaches a call loop.
    Skipped,
}

/// Returns `models` ordered so each function comes after every function it
/// calls.
///
/// Only direct calls (`call imm`) between functions in `models` count.
pub(super) fn callees_first(models: &[FunctionModel]) -> Vec<ModelIndex> {
    // Quickly map addr to index.
    let mut address_to_index_map = HashMap::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        address_to_index_map.insert(model.address, ModelIndex::from(index));
    }

    // List the functions each function calls.
    let mut callees = vec![Vec::new(); models.len()];
    for (caller, model) in models.iter().enumerate() {
        for op in model.blocks.iter().flat_map(|block| &block.ops) {
            if let Op::Call(call) = op
                && let Some(target) = direct_call_target(&call.guest)
                && let Some(&callee) = address_to_index_map.get(&target)
            {
                callees[caller].push(callee);
            }
        }
    }

    // Now make the final list/order.
    let mut progress = vec![Progress::NotSeen; models.len()];
    let mut order = Vec::with_capacity(models.len());
    for function in (0..models.len()).map(ModelIndex::from) {
        add(function, &callees, &mut progress, &mut order);
    }
    order
}

/// Adds everything `function` calls to `order`, then `function` itself.
///
/// # Returns
///
/// `false` if `function` reaches a call loop, so we left it out.
fn add(
    function: ModelIndex,
    callees: &[Vec<ModelIndex>],
    progress: &mut [Progress],
    order: &mut Vec<ModelIndex>,
) -> bool {
    match progress[function] {
        Progress::Added => return true,
        Progress::AddingCallees | Progress::Skipped => return false,
        Progress::NotSeen => {}
    }

    progress[function] = Progress::AddingCallees;
    let callees_added = callees[function]
        .iter()
        .all(|&callee| add(callee, callees, progress, order));

    if callees_added {
        progress[function] = Progress::Added;
        order.push(function);
    } else {
        progress[function] = Progress::Skipped;
    }
    callees_added
}
