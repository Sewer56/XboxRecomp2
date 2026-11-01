//! Stores many short lists end to end in one buffer.

use core::ops::Range;

/// Many short lists, stored end to end in one buffer.
///
/// For example, these 5 lists:
///
/// ```text
/// lists[0]: []
/// lists[1]: [0]
/// lists[2]: [1]
/// lists[3]: [1, 2]
/// lists[4]: [3]
/// ```
///
/// Are stored as:
///
/// ```text
/// items:  [0, 1, 1, 2, 3]
/// starts: [0, 0, 1, 2, 4, 5]
/// ```
///
/// Each list ends where the next one starts.
///
/// So `lists[3]` is `items[starts[3]..starts[4]]`.
/// That's `items[2..4]`, or `[1, 2]`.
///
/// And one extra item in `starts` to mark end of last list.
///
/// Build one with [`FlatListsBuilder`].
pub(crate) struct FlatLists<T> {
    /// Every list, end to end.
    items: Box<[T]>,

    /// Where each list starts in `items`.
    starts: Box<[u32]>,
}

/// Adds lists one at a time, then turns them into [`FlatLists`].
pub(crate) struct FlatListsBuilder<T> {
    items: Vec<T>,
    starts: Vec<u32>,
}

impl<T> FlatLists<T> {
    /// Returns the list at `index`.
    pub(crate) fn get(&self, index: usize) -> &[T] {
        &self.items[self.range(index)]
    }

    /// Where the list at `index` sits among all items.
    pub(crate) fn range(&self, index: usize) -> Range<usize> {
        let start = self.starts[index] as usize;
        let end = self.starts[index + 1] as usize; // <- ends where the next list starts
        start..end
    }

    /// Number of lists.
    pub(crate) fn list_count(&self) -> usize {
        self.starts.len() - 1
    }

    /// Number of items across all lists.
    pub(crate) fn item_count(&self) -> usize {
        self.items.len()
    }
}

impl<T> FlatListsBuilder<T> {
    /// Creates a builder with no lists.
    ///
    /// # Arguments
    ///
    /// - `lists`: expected number of lists.
    /// - `items`: expected number of items across all lists.
    pub(crate) fn with_capacity(lists: usize, items: usize) -> Self {
        let mut starts = Vec::with_capacity(lists + 1);
        starts.push(0); // <- where the first list starts
        Self {
            items: Vec::with_capacity(items),
            starts,
        }
    }

    /// Adds `list` after the last list.
    pub(crate) fn push(&mut self, list: impl IntoIterator<Item = T>) {
        self.items.extend(list);
        self.starts.push(self.items.len() as u32);
    }

    /// Returns the finished lists.
    pub(crate) fn build(self) -> FlatLists<T> {
        FlatLists {
            items: self.items.into_boxed_slice(),
            starts: self.starts.into_boxed_slice(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example from docs
    #[test]
    fn basic_test() {
        // Arrange
        let lists: [&[u32]; 5] = [&[], &[0], &[1], &[1, 2], &[3]];
        let mut builder = FlatListsBuilder::with_capacity(lists.len(), 5);

        // Act
        for list in lists {
            builder.push(list.iter().copied());
        }
        let flat = builder.build();

        // Assert
        assert_eq!(*flat.items, [0, 1, 1, 2, 3]);
        assert_eq!(*flat.starts, [0, 0, 1, 2, 4, 5]);

        assert_eq!(flat.range(3), 2..4);
        assert!(flat.get(0).is_empty());
        assert_eq!(flat.get(3), [1, 2]);
        assert_eq!(flat.list_count(), 5);
        assert_eq!(flat.item_count(), 5);
    }
}
