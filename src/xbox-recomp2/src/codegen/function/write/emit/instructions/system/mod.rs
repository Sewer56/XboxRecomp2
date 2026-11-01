//! System instructions. Most of these write nothing.

#[path = "in.rs"]
pub(super) mod r#in;
pub(super) mod nop;
pub(super) mod out;
pub(super) mod prefetcht0;
pub(super) mod rdtsc;
pub(super) mod sfence;
pub(super) mod sgdt;
pub(super) mod sldt;
pub(super) mod wbinvd;
