//! Resolves labels and blocks to instruction indices.

use super::super::{Exit, FunctionModel};
use std::collections::BTreeMap;
use xbox_recomp2_executable::GuestAddress32;

/// Resolves labels and blocks to instruction indices.
pub(super) struct OpResolver<'a> {
    model: &'a FunctionModel,

    /// Block index for each label.
    blocks_by_label: BTreeMap<LabelAddress, BlockIndex>,

    /// Index of each block's first instruction.
    ///
    /// Index == Block Index
    /// Value == Index of the block's first instruction.
    pub(super) starts: Vec<u32>,

    /// Total instructions across all blocks.
    pub(super) instruction_count: u32,
}

/// A block's position in [`FunctionModel::blocks`].
#[derive(Clone, Copy)]
pub(super) struct BlockIndex(pub(super) usize);

/// The guest address a block starts at, which jumps use as their target.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LabelAddress(pub(super) GuestAddress32);

impl<'a> OpResolver<'a> {
    /// Indexes the labels and blocks of `model`.
    pub(super) fn new(model: &'a FunctionModel) -> Self {
        let blocks_by_label = model
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                block
                    .label
                    .map(|label| (LabelAddress(label), BlockIndex(index)))
            })
            .collect();

        let mut starts = Vec::with_capacity(model.blocks.len());
        let mut instruction_count = 0u32;
        for block in &model.blocks {
            starts.push(instruction_count);
            instruction_count += block.ops.len() as u32;
        }

        Self {
            model,
            blocks_by_label,
            starts,
            instruction_count,
        }
    }

    /// Returns the block a label points at.
    pub(super) fn block_at(&self, label: LabelAddress) -> Option<BlockIndex> {
        self.blocks_by_label.get(&label).copied()
    }

    /// Returns the first instruction that runs when we enter `block`.
    pub(super) fn first_op(&self, mut block: BlockIndex) -> Option<u32> {
        for _ in 0..self.model.blocks.len() {
            let current = self.model.blocks.get(block.0)?;
            if !current.ops.is_empty() {
                return Some(self.starts[block.0]);
            }

            block = match current.exit {
                Exit::Next => BlockIndex(block.0 + 1),
                Exit::Goto(label) => self.block_at(LabelAddress(label))?,
                _ => return None,
            };
        }
        None
    }

    /// Returns the first instruction that runs when we jump to `label`.
    pub(super) fn op_at(&self, label: LabelAddress) -> Option<u32> {
        self.block_at(label).and_then(|block| self.first_op(block))
    }
}
