//! Targets learned for a function and kept across its walks.

use crate::analysis::graph::{FunctionReference, LocalEdge};
use std::collections::BTreeSet;

/// Targets saved for later walks of one function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::analysis) struct LearnedTargets {
    /// Paths inside the function, such as switch cases.
    pub edges: BTreeSet<LocalEdge>,

    /// Pointers, register calls, and register jumps to other functions.
    pub references: BTreeSet<FunctionReference>,
}
