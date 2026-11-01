//! Lists the operations that can run right before each operation.

use crate::utils::flat_lists::{FlatLists, FlatListsBuilder};

/// Operations that can run right before each operation.
pub(super) struct Predecessors {
    lists: FlatLists<u32>,
}

impl Predecessors {
    /// Builds the lists.
    ///
    /// # Arguments
    ///
    /// - `count`: number of operations.
    /// - `links`: each `(from, to)` pair where `to` can run right after `from`.
    pub(super) fn new(count: usize, links: impl IntoIterator<Item = (u32, u32)>) -> Self {
        // Step 1: give each operation its own list.
        //
        // grouped is a `to -> from` mapping
        // grouped[0]: []
        // grouped[1]: [0]
        // grouped[2]: [1]
        // grouped[3]: [1, 2]
        // grouped[4]: [3]
        let mut grouped = vec![Vec::new(); count];
        for (from, to) in links {
            grouped[to as usize].push(from);
        }

        // Step 2: yeet off to builder ye go.
        let total = grouped.iter().map(Vec::len).sum();
        let mut lists = FlatListsBuilder::with_capacity(count, total); // <- count == list_count , container automatically handles `starts`
        for previous in grouped {
            lists.push(previous);
        }
        Self {
            lists: lists.build(),
        }
    }

    /// Operations that can run right before the one at `index`.
    pub(super) fn of(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let previous = self.lists.get(index);
        previous.iter().map(|&operation| operation as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example from docs
    #[test]
    fn new_should_group_links_by_target() {
        // Arrange
        let links = [
            (0, 1), // add  -> je
            (1, 3), // je   -> sets  (taken)
            (1, 2), // je   -> adc   (not taken)
            (2, 3), // adc  -> sets
            (3, 4), // sets -> ret
        ];

        // Act
        let predecessors = Predecessors::new(5, links);

        // Assert
        assert_eq!(predecessors.of(0).count(), 0); // <- add runs first
        assert_eq!(predecessors.of(3).collect::<Vec<_>>(), [1, 2]); // <- both je paths
        assert_eq!(predecessors.of(4).collect::<Vec<_>>(), [3]);
    }
}
