//! Shared helpers for core integration tests.

pub(crate) use platform::PLATFORM;
pub(crate) use smoke::smoke_text;
pub(crate) use synthetic::Synthetic;
pub(crate) use temp_dir::temp_dir;

mod platform;
mod smoke;
mod synthetic;
mod temp_dir;
