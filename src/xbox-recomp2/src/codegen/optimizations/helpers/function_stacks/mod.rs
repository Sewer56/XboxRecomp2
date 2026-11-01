//! Tracks the stack of every function, so we can step over calls between them.
//!
//! See `function-stacks.md` for more info.
//!
//! - [`FunctionStacks::build`]: walk every function, callees first.
//! - [`FunctionStacks::get`]: get one function's stack.

use super::function_stack::{CallEffect, FunctionStack};
use crate::codegen::function::model::FunctionModel;
use crate::config::FunctionAddress;
use call_graph::callees_first;
use std::collections::BTreeMap;

mod call_graph;
mod effects;

/// Each function's stack, and what calling it does to ESP and EBP.
///
/// This is a snapshot. Rebuild it after changing any model.
pub(in crate::codegen) struct FunctionStacks<'a> {
    entries: BTreeMap<FunctionAddress, Entry<'a>>,
}

/// What we know about one function.
struct Entry<'a> {
    /// ESP and EBP at each instruction inside the function.
    /// [`None`] if we only trust its `ret N`.
    stack: Option<FunctionStack<'a>>,

    /// ESP and EBP after a call to the function returns.
    effect: CallEffect,
}

impl<'a> FunctionStacks<'a> {
    /// Walks every function, after the functions it calls.
    ///
    /// # Remarks
    ///
    /// With `trust_returns`, we trust the `ret N` of functions we can't walk.
    /// An unsafe optimisation, but often still a-ok.
    pub(in crate::codegen) fn build(models: &'a [FunctionModel], trust_returns: bool) -> Self {
        let mut result = Self {
            entries: BTreeMap::new(),
        };

        // Get all functions, sorted by callees first, then callers.
        for index in callees_first(models) {
            let model = &models[index];
            if let Some(entry) = result.entry_for(model, trust_returns) {
                result.entries.insert(model.address, entry);
            }
        }
        result
    }

    /// Works out one function's entry, using the entries of its callees.
    ///
    /// Returns [`None`] if the host may replace the function,
    /// or we can't work out what calling it does.
    fn entry_for(&self, model: &'a FunctionModel, trust_returns: bool) -> Option<Entry<'a>> {
        if !model.can_change_abi() {
            return None;
        }
        let calls = |target| Some(self.entries.get(&target)?.effect);
        let stack = FunctionStack::new(model, calls);
        let effect = match &stack {
            Some(stack) => effects::from_walk(stack, calls),
            None if trust_returns => effects::trusting_ret_n(model),
            None => None,
        }?;
        Some(Entry { stack, effect })
    }

    /// Returns the stack of the function at `address`.
    ///
    /// Returns [`None`] if we couldn't walk it.
    /// That includes functions where we only trust their `ret N`.
    pub(in crate::codegen) fn get(&self, address: FunctionAddress) -> Option<&FunctionStack<'a>> {
        self.entries.get(&address)?.stack.as_ref()
    }
}
