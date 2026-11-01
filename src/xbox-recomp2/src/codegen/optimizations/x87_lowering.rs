//! Turns x87 stack code into plain C variables.
//!
//! The values live in locals, e.g.
//! ```c
//! double st0;
//! double st1;
//! ```
//!
//! The `ctx.fpu` is written back only where other code can see it: calls,
//! returns and hooks.
//!
//! Currently stubbed out.

use crate::codegen::function::model::FunctionModel;

/// Rewrites one function's x87 code to use locals.
pub(super) fn run(_model: &mut FunctionModel) {}
