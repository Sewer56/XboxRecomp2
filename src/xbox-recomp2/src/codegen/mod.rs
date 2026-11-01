//! Turns analysed functions into C++ source files.
//!
//! Three [`Recompiler`] methods run generation:
//!
//! - [`Recompiler::recompile`] writes the files to disk.
//! - [`Recompiler::generate_in_memory`] keeps the files in memory instead.
//! - [`Recompiler::generate`] does the actual work for both.
//!
//! What lives here:
//!
//! - [`function`]: writes the C++ for one function.
//! - [`optimizations`]: runs the optimisations, and holds the helpers they share.
//! - [`run_state`]: the reports and counters shared by every function.
//! - [`templates`]: text snippets for the generated code.
//!
//! Instruction emission lives beneath [`function`]'s writer. File storage
//! and shared file rendering live in [`crate::output`].
//! [`Recompiler::generate`] coordinates them.
//!
//! [`Recompiler`]: crate::Recompiler
//! [`Recompiler::recompile`]: crate::Recompiler::recompile
//! [`Recompiler::generate_in_memory`]: crate::Recompiler::generate_in_memory
//! [`Recompiler::generate`]: crate::Recompiler::generate
//! [`templates`]: crate::templates
pub(crate) mod function;
pub(crate) mod optimizations;
pub(crate) mod run_state;
pub mod templates;
