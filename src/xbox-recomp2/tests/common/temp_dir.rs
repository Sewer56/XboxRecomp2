//! Temp directories that don't clash when tests run in parallel.

use core::sync::atomic::{AtomicUsize, Ordering};
use std::fs;
use std::path::PathBuf;

/// Creates a new, empty temp directory and returns its path.
///
/// The name includes `name`, the process ID and a counter, so parallel
/// tests never share one.
///
/// # Panics
///
/// Panics if creating the directory fails.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "xboxrecomp2-core-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).expect("create temp dir");
    dir
}
