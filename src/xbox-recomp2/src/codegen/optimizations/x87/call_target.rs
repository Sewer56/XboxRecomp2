//! Finds who each call and tail call goes to.

use super::Hacks;
use crate::codegen::function::model::{Call, FunctionModel};
use crate::codegen::optimizations::helpers::known_callers::direct_call_target;
use crate::codegen::optimizations::helpers::model_index::ModelIndex;
use crate::config::FunctionAddress;
use crate::decode::{immediate, memory};
use crate::executable::ImportMap;
use ahash::{HashMap, HashMapExt};
use xbox_recomp2_executable::GuestAddress32;

/// Who a call or tail call goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CallTarget {
    /// A guest function we generate.
    Function(ModelIndex),

    /// An OS function: host code that leaves the x87 state alone.
    Import,

    /// A target only known at run time, like `call [eax+0x10]`.
    Indirect,

    /// An address that isn't a function we know.
    Unknown,
}

/// Looks up call targets.
pub(super) struct CallTargets<'a> {
    index_of: HashMap<FunctionAddress, ModelIndex>,
    imports: &'a ImportMap,

    /// The speed hacks, carried along for the count.
    pub hacks: Hacks,
}

impl<'a> CallTargets<'a> {
    /// Indexes every function in `models`.
    pub(super) fn new(models: &[FunctionModel], imports: &'a ImportMap, hacks: Hacks) -> Self {
        let mut index_of = HashMap::with_capacity(models.len());
        for (index, model) in models.iter().enumerate() {
            index_of.insert(model.address, ModelIndex::from(index));
        }

        Self {
            index_of,
            imports,
            hacks,
        }
    }

    /// Who `call` goes to.
    pub(super) fn find_call_target(&self, call: &Call) -> CallTarget {
        // `call 0x1000`.
        if let Some(target) = direct_call_target(&call.guest) {
            return self.find_function(target);
        }

        // Far and 16-bit calls the emitter can't write.
        let operand = &call.guest.insn.operands()[0];
        if immediate(operand).is_some() {
            return CallTarget::Unknown;
        }

        // `call [thunk]` to an OS function.
        if let Some(mem) = memory(operand)
            && self
                .imports
                .resolve_thunk((mem.disp.displacement as u32).into())
                .is_some()
        {
            return CallTarget::Import;
        }

        CallTarget::Indirect
    }

    /// Who a jump out of the function to `target` goes to.
    pub(super) fn find_jump_target(&self, target: GuestAddress32) -> CallTarget {
        self.find_function(FunctionAddress(target.get()))
    }

    fn find_function(&self, target: FunctionAddress) -> CallTarget {
        match self.index_of.get(&target) {
            Some(&index) => CallTarget::Function(index),
            None => CallTarget::Unknown,
        }
    }
}
