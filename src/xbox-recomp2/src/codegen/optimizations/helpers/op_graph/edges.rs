//! Finds every way each op in a function can continue.

use super::Edge;
use super::op_resolver::{BlockIndex, LabelAddress, OpResolver};
use crate::codegen::function::model::{Exit, FunctionModel, GuestOp, MergedInstructionKind, Op};
use crate::config::Register as ConfigRegister;
use crate::decode::{
    BranchType, InstructionCategory, Mnemonic, Register, immediate, memory, register,
};
use crate::executable::SwitchTable;
use crate::utils::flat_lists::{FlatLists, FlatListsBuilder};
use xbox_recomp2_executable::GuestAddress32;

/// Finds every way each op in `model` can continue.
///
/// # Returns
///
/// Each op's edges, then the ops those edges go to.
pub(super) fn find(
    model: &FunctionModel,
    resolver: &OpResolver,
) -> (FlatLists<Edge>, FlatLists<u32>) {
    let count = resolver.instruction_count as usize;
    let mut op_targets = FlatListsBuilder::with_capacity(count, count);
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
                Op::Call(_) | Op::CdqIdiv(_) => edges.push(next),
                Op::MergedCmpJmp(merged) => {
                    for instruction in &merged.instructions {
                        if let MergedInstructionKind::Jump(_) = instruction.kind {
                            edges.push(branch_edge(&instruction.guest, resolver));
                        }
                    }
                    edges.push(next);
                }
                Op::Guest(guest) => match guest.insn.instruction().meta.category {
                    InstructionCategory::RET => {}
                    InstructionCategory::UNCOND_BR if guest.switch_table.is_some() => {
                        add_switch(&mut edges, guest, resolver);
                    }
                    InstructionCategory::COND_BR | InstructionCategory::UNCOND_BR => {
                        edges.push(branch_edge(guest, resolver));
                        if guest.insn.mnemonic() != Mnemonic::JMP {
                            edges.push(next);
                        }
                    }
                    _ => edges.push(next),
                },
            }

            // Add edge to next instruction, and next location.
            op_targets.push(edges.iter().filter_map(|edge| match *edge {
                Edge::Op(target) => Some(target),
                _ => None,
            }));
            all_edges.push(edges.iter().copied());
        }
    }

    (all_edges.build(), op_targets.build())
}

/// Adds each case of the switch `jmp` in `guest` to `edges`.
///
/// Cases outside the function become [`Edge::Function`].
///
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
