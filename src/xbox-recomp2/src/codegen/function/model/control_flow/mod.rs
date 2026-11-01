//! Maps each instruction in a function to the instructions that can run next.
//!
//! - [`ControlFlow::new`] builds the map for a [`FunctionModel`].
//! - [`ControlFlow::successors`] returns where one instruction can go next.
//! - [`ControlFlow::edges`] returns every way one instruction can continue,
//!   including jumps out of the function.

use super::{Entry, Exit, FunctionModel, GuestOp, MergedInstructionKind, Op};
use crate::config::Register as ConfigRegister;
use crate::decode::{
    BranchType, InstructionCategory, Mnemonic, Register, immediate, memory, register,
};
use crate::executable::SwitchTable;
use crate::utils::flat_lists::{FlatLists, FlatListsBuilder};
use core::ops::Range;
use op_resolver::{BlockIndex, LabelAddress, OpResolver};
use xbox_recomp2_executable::GuestAddress32;

mod op_resolver;

/// Where each instruction in a [`FunctionModel`] can go next.
pub(in crate::codegen) struct ControlFlow {
    /// The first instruction to run.
    ///
    /// In the rare cases where first/lowest address is not the function's
    /// entry (i.e. a jump backwards)
    pub entry: u32,

    /// Each instruction's next instructions.
    successors: FlatLists<u32>,

    /// Each instruction's edges, the paths the instruction can continue.
    edges: FlatLists<Edge>,
}

/// One way an instruction can continue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codegen) enum Edge {
    /// Runs the instruction at this index next.
    Op(u32),

    /// Jumps to code outside this function, like a tail call to `0x2000`.
    Function(GuestAddress32),

    /// Aborts, so no guest code runs after it.
    Abort,

    /// An indirect jump, like `jmp eax`, to code we can't name.
    Indirect,

    /// We don't know what runs next, such as after a decode failure.
    Unknown,
}

impl ControlFlow {
    /// Builds the control flow map for `model`.
    pub(in crate::codegen) fn new(model: &FunctionModel) -> Option<Self> {
        let resolver = OpResolver::new(model);

        // Go to first instruction in method.
        let entry = resolver.first_op(match model.entry {
            Entry::First => BlockIndex(0),
            Entry::Goto(label) => resolver.block_at(LabelAddress(label))?,
            Entry::Missing(_) => return None,
        })?;

        let count = resolver.instruction_count as usize;
        let mut successors = FlatListsBuilder::with_capacity(count, count);
        let mut all_edges = FlatListsBuilder::with_capacity(count, count);
        let mut edges = Vec::with_capacity(2); // <- most instructions have 1 or 2

        // Go over the block and go through each operation.
        for (block_index, block) in model.blocks.iter().enumerate() {
            // Where the block's last instruction goes if no jmp/ret.
            let exit = match block.exit {
                Exit::Next => resolver
                    .first_op(BlockIndex(block_index + 1))
                    .map_or(Edge::Unknown, Edge::Op),
                Exit::Goto(label) => resolver
                    .op_at(LabelAddress(label))
                    .map_or(Edge::Unknown, Edge::Op),
                Exit::Abort { .. } | Exit::NoFallthrough { .. } => Edge::Abort,
            };

            for (index, op) in block.ops.iter().enumerate() {
                // Get 1st instruction.
                // 『空FC』は神ゲーだ。
                let next = if index + 1 < block.ops.len() {
                    Edge::Op(resolver.starts[block_index] + index as u32 + 1)
                } else {
                    exit
                };

                // Collect where this instruction can go.
                edges.clear();
                match op {
                    Op::Failed { .. } => edges.push(Edge::Unknown),
                    Op::Return(_) => {}
                    Op::Call(_) => edges.push(next),
                    Op::MergedCmpJmp(merged) => {
                        for instruction in &merged.instructions {
                            if let MergedInstructionKind::Jump(_) = instruction.kind {
                                edges.push(branch_edge(&instruction.guest, &resolver));
                            }
                        }
                        edges.push(next);
                    }
                    Op::Guest(guest) => match guest.insn.instruction().meta.category {
                        InstructionCategory::RET => {}
                        InstructionCategory::UNCOND_BR if guest.switch_table.is_some() => {
                            add_switch(&mut edges, guest, &resolver);
                        }
                        InstructionCategory::COND_BR | InstructionCategory::UNCOND_BR => {
                            edges.push(branch_edge(guest, &resolver));
                            if guest.insn.mnemonic() != Mnemonic::JMP {
                                edges.push(next);
                            }
                        }
                        _ => edges.push(next),
                    },
                }

                // Add edge to next instruction, and next location.
                successors.push(edges.iter().filter_map(|edge| match *edge {
                    Edge::Op(target) => Some(target),
                    _ => None,
                }));
                all_edges.push(edges.iter().copied());
            }
        }

        Some(Self {
            entry,
            successors: successors.build(),
            edges: all_edges.build(),
        })
    }

    /// Returns the indices of the instructions that can run after `index`.
    ///
    /// # Returns
    ///
    /// - Empty for a return.
    /// - One index if it always goes to the same place.
    /// - For a conditional jump, the jump target, then the next instruction.
    /// - For a switch, each distinct case, in index order.
    /// - [`None`] if unknown, or it leaves the function without returning.
    pub(in crate::codegen) fn successors(&self, index: usize) -> Option<&[u32]> {
        let local = self
            .edges(index)
            .iter()
            .all(|edge| matches!(edge, Edge::Op(_)));
        local.then(|| self.successors.get(index))
    }

    /// Returns every way the instruction at `index` can continue.
    ///
    /// # Returns
    ///
    /// - Empty for a return.
    /// - For a conditional jump, the jump's edge, then the next instruction's.
    /// - For a merged cmp/jmp, each merged jump's edge in order, then the next
    ///   instruction's.
    /// - For a switch, each distinct local case, then any case leaving the function.
    pub(in crate::codegen) fn edges(&self, index: usize) -> &[Edge] {
        self.edges.get(index)
    }

    /// Where the edges of the instruction at `index` sit among all edges.
    pub(in crate::codegen) fn edge_range(&self, index: usize) -> Range<usize> {
        self.edges.range(index)
    }

    /// Number of instructions in the map.
    pub(in crate::codegen) fn len(&self) -> usize {
        self.edges.list_count()
    }

    /// Number of edges across all instructions.
    pub(in crate::codegen) fn edges_len(&self) -> usize {
        self.edges.item_count()
    }
}

/// Adds each case of the switch `jmp` in `guest` to `edges`.
///
/// Cases outside the function become [`Edge::Function`].
/// If the `jmp` doesn't use the table's index register, or the table is empty,
/// adds one [`Edge::Indirect`] instead.
fn add_switch(edges: &mut Vec<Edge>, guest: &GuestOp, resolver: &OpResolver) {
    let Some(table) = guest
        .switch_table
        .as_ref()
        .filter(|table| switch_matches(guest, table))
    else {
        edges.push(Edge::Indirect);
        return;
    };

    // Note: Tables can repeat cases, such as the default.
    let mut cases: Vec<_> = table
        .labels
        .iter()
        .map(|&label| {
            let label = GuestAddress32::from(label);
            resolver
                .op_at(LabelAddress(label))
                .map_or(Edge::Function(label), Edge::Op)
        })
        .collect();

    cases.sort_unstable_by_key(|edge| match *edge {
        Edge::Op(target) => (0, target),
        Edge::Function(address) => (1, address.get()),
        _ => (2, 0),
    });

    cases.dedup();
    if cases.is_empty() {
        cases.push(Edge::Indirect);
    }

    edges.extend_from_slice(&cases);
}

/// Where a direct or indirect jump in `guest` goes.
fn branch_edge(guest: &GuestOp, resolver: &OpResolver) -> Edge {
    let Some(target) = direct_branch_target(guest) else {
        let operand = &guest.insn.operands()[0];
        let indirect = register(operand).is_some() || memory(operand).is_some();
        return if indirect {
            Edge::Indirect
        } else {
            Edge::Unknown
        };
    };

    resolver
        .op_at(LabelAddress(target))
        .map_or(Edge::Function(target), Edge::Op)
}

/// Returns the address a jump lands on, such as `0x1010` for `jnz 0x1010`.
///
/// Returns [`None`] for jumps we don't follow:
/// - Indirect jumps, like `jmp eax`.
/// - Far jumps.
/// - 16-bit jumps.
fn direct_branch_target(guest: &GuestOp) -> Option<GuestAddress32> {
    let insn = guest.insn.instruction();
    let displacement = immediate(&guest.insn.operands()[0])?;
    if insn.operand_width != 32
        || !displacement.is_relative
        || !matches!(insn.meta.branch_type, BranchType::SHORT | BranchType::NEAR)
    {
        return None;
    }

    // x86 measures the jump from the end of the instruction.
    // A 2-byte `jz +4` at 0x1001
    // -> ins ends on 0x1003
    // -> +4 offset
    // == 0x1007
    Some(
        guest
            .address
            .wrapping_add(guest.insn.length() as u32)
            .wrapping_add(displacement.value as u32),
    )
}

/// Whether `table` describes the `jmp [table + index * 4]` in `guest`.
fn switch_matches(guest: &GuestOp, table: &SwitchTable) -> bool {
    let insn = &guest.insn;
    let index = match table.register {
        ConfigRegister::Eax => Register::EAX,
        ConfigRegister::Ebx => Register::EBX,
        ConfigRegister::Ecx => Register::ECX,
        ConfigRegister::Edx => Register::EDX,
        ConfigRegister::Esi => Register::ESI,
        ConfigRegister::Edi => Register::EDI,
        ConfigRegister::Ebp => Register::EBP,
        ConfigRegister::Esp => Register::ESP,
    };
    insn.mnemonic() == Mnemonic::JMP
        && insn.instruction().operand_width == 32
        && memory(&insn.operands()[0]).is_some_and(|mem| mem.index == index)
}
