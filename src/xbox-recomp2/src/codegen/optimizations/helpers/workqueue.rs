//! A queue of items to visit, each queued at most once.
//!
//! Items are indices into some collection.
//! So `full(10)` queues items (indices) `0` through `9`.

use core::mem;

/// Items waiting to be visited.
///
/// # Remarks
///
/// Pushing an item that is already waiting does nothing.
pub(in crate::codegen) struct WorkQueue<I> {
    pending: Vec<I>,
    queued: Vec<bool>,
}

impl<I: Copy + From<usize> + Into<usize>> WorkQueue<I> {
    /// Queues every item, from `0` up to `len`.
    pub(in crate::codegen) fn full(len: usize) -> Self {
        Self {
            pending: (0..len).map(I::from).collect(),
            queued: vec![true; len],
        }
    }

    /// Takes the next item, or [`None`] when the queue is empty.
    pub(in crate::codegen) fn pop(&mut self) -> Option<I> {
        let index = self.pending.pop()?;
        self.queued[index.into()] = false;
        Some(index)
    }

    /// Queues `index` unless it is already waiting.
    ///
    /// # Panics
    ///
    /// If `index` is not below the `len` given to [`WorkQueue::full`].
    pub(in crate::codegen) fn push(&mut self, index: I) {
        if !mem::replace(&mut self.queued[index.into()], true) {
            self.pending.push(index);
        }
    }
}
