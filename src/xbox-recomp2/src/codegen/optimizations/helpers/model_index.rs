//! A function's position in the model list.

use core::ops::{Index, IndexMut};

/// A function's position in a `[FunctionModel]` slice.
///
/// # Remarks
///
/// Slices and [`Vec`]s take it as an index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::codegen) struct ModelIndex(usize);

impl From<usize> for ModelIndex {
    fn from(index: usize) -> Self {
        Self(index)
    }
}

impl From<ModelIndex> for usize {
    fn from(index: ModelIndex) -> Self {
        index.0
    }
}

impl<T> Index<ModelIndex> for [T] {
    type Output = T;

    fn index(&self, index: ModelIndex) -> &T {
        &self[index.0]
    }
}

impl<T> IndexMut<ModelIndex> for [T] {
    fn index_mut(&mut self, index: ModelIndex) -> &mut T {
        &mut self[index.0]
    }
}

impl<T> Index<ModelIndex> for Vec<T> {
    type Output = T;

    fn index(&self, index: ModelIndex) -> &T {
        &self[index.0]
    }
}

impl<T> IndexMut<ModelIndex> for Vec<T> {
    fn index_mut(&mut self, index: ModelIndex) -> &mut T {
        &mut self[index.0]
    }
}
