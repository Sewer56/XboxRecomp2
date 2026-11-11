//! The function model that optimisation passes read and change.
//!
//! - [`build`] turns one guest function into a [`FunctionModel`].
//! - Optimisation passes rewrite it.
//! - [`write`] turns it into C++.
//!
//! The model is a list of blocks. Each block has an optional label, a
//! list of operations, and a way out. Most operations are decoded guest
//! instructions.
//!
//! Passes can replace some with their own kinds for specialised code
//! generation.
//!
//! A block starts wherever a branch can land.
//! Example:
//!
//! ```text
//! block 0                      ; no label: nothing jumps here
//!   op: mov eax, [0x5356F0]
//!   op: test eax, eax
//!   op: jnz 0x11423
//!   op: call 0x113B0          ; stored as `Call`
//!   op: mov [0x5356F0], eax
//!   exit: next block
//! block 1  (label: loc_11423)  ; the `jnz` lands here
//!   op: ret                   ; stored as `Return`
//!   exit: next block
//! ```
//!
//! [`build`]: super::build
//! [`write`]: mod@super::write

pub(in crate::codegen) use super::build::StepFailure;
use crate::analysis::LocalTargets;
use crate::codegen::optimizations::helpers::instruction::effects::x87_status::ConditionCodes;
use crate::config::FunctionAddress;
use crate::decode::{CpuFlag, Insn, Register};
use crate::executable::SwitchTable;
pub(crate) use hook::Hook;
pub(crate) use intrinsics::{
    Call, CdqIdiv, Condition, MergedCmpJmp, MergedInstruction, MergedInstructionKind, Return,
};
pub(crate) use x87::{HookSync, X87Level, X87Plan, X87Sync};
use xbox_recomp2_executable::GuestAddress32;

mod hook;
mod intrinsics;
mod x87;

/// Flags stored in `X86EFlags::common`.
///
/// Keep in sync with `X86EFlags::ARITHMETIC_FLAGS` in `x86_context_core.h`.
pub(in crate::codegen) const ALL_ARITHMETIC_FLAGS: CpuFlag = CpuFlag::from_bits_retain(0x8D5);

/// A position of an instruction in a block's ([`Block::ops`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::codegen) struct BlockOpIndex(usize);

/// One guest function, ready for passes and emission.
#[derive(Clone, Debug, Default)]
pub(crate) struct FunctionModel {
    /// Where the function starts in guest memory.
    pub address: FunctionAddress,

    /// The blocks, in the order they are written.
    pub blocks: Vec<Block>,

    /// The mid-assembly hooks, in first-seen order.
    ///
    /// Each one gets an `extern` declaration above the function.
    pub hooks: Vec<Hook>,

    /// How the function starts.
    pub entry: Entry,

    /// Whether jmp(s) with a 16-bit operand keep only the target's low 16 bits.
    pub mask_branch_targets: bool,

    /// Whether a host function may replace this one.
    ///
    /// Passes leave host replacements on their original calling convention.
    ///
    /// For example, the stack argument elimination pass doesn't pass arguments
    /// in registers, because the host's replacement would still read them from
    /// the stack.
    ///
    /// See [`can_change_abi`].
    ///
    /// [`can_change_abi`]: Self::can_change_abi
    pub abi: FunctionAbi,

    /// What's changed from original calling convention.
    pub fastcall: FastcallConvention,

    /// Whether `[ebp+N]` always points into this function's stack frame.
    pub ebp_frame: bool,

    /// How to write the x87 code, set by the `optimize_x87` pass.
    pub x87: X87Plan,

    /// Whether MMX registers live in a local `uint64_t mmx[8]`.
    ///
    /// Set by the `optimize_mmx` pass.
    pub local_mmx: bool,
}

/// A scratch variable declared at the top of the function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Local {
    /// `x87_float val_double_{index} = 0;`
    Double(u8),

    /// `uint32_t val_uint32_{index} = 0;`
    Uint32(u8),

    /// `uint64_t val_uint64_{index} = 0;`
    Uint64(u8),
}

/// A straight run of operations.
#[derive(Clone, Debug)]
pub(crate) struct Block {
    /// The guest address branches use to jump here, if any.
    ///
    /// Written as `loc_<address>:`.
    pub label: Option<GuestAddress32>,

    /// The operations, in order.
    pub ops: Vec<Op>,

    /// What runs after the last operation.
    pub exit: Exit,
}

/// How the generated function starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Entry {
    /// The first written block is the entry.
    #[default]
    First,

    /// `goto` the block labelled with this guest address first.
    Goto(GuestAddress32),

    /// The entry has no instruction; abort right away.
    Missing(GuestAddress32),
}

/// What optimizations changed in a function's calling convention.
#[derive(Clone, Debug, Default)]
pub(crate) struct FastcallConvention {
    /// Stack arguments the `_fastcall` entry takes in registers,
    /// as `(argument, register)`.
    pub register_arguments: Vec<(u8, Register)>,

    /// Whether the `_fastcall` entry takes no return address.
    pub omits_return_slot: bool,
}

/// Whether host replacements and parameter changes are allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FunctionAbi {
    /// Regular functions. Runs, can be inlined, we ballin'.
    #[default]
    Original,

    /// Listed in `fixes.function_hooks`, so host replaces it.
    Replaced,

    /// An optimization changed how the function is called.
    /// Calling convention was broken, so we refuse calls to this
    /// code from the recompilation.
    Protected,
}

/// What runs after a block's last operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The next written block. The last operation may also leave the
    /// function on its own, like `ret`.
    Next,

    /// `goto` the block labelled with this guest address.
    Goto(GuestAddress32),

    /// Abort, because the block doesn't jump or return.
    ///
    /// Like [`Abort`], but the writer picks the message from
    /// the last operation, such as `INT3 reached`.
    ///
    /// Supposed to be unreachable code. If reached, we messed up.
    ///
    /// [`Abort`]: Self::Abort
    NoFallthrough {
        /// The guest address the message names.
        address: GuestAddress32,
    },

    /// Print this message and abort, so control can't run into the
    /// next block by accident.
    Abort {
        /// What went wrong.
        message: &'static str,

        /// The guest address the message names.
        address: GuestAddress32,
    },
}

/// One operation in a block.
#[derive(Clone, Debug)]
pub(crate) enum Op {
    /// A guest instruction, written by its emitter.
    Guest(GuestOp),

    /// A decoded call and its current calling convention.
    Call(Call),

    /// A merged 'cmp' and 'jmp' operation.
    MergedCmpJmp(Box<MergedCmpJmp>),

    /// A decoded return and its current stack cleanup.
    Return(Return),

    /// An `idiv r/m32` after `cdq`.
    CdqIdiv(CdqIdiv),

    /// Usually a decode error.
    Failed {
        /// Where the instruction starts.
        address: GuestAddress32,

        /// Why it couldn't be decoded.
        failure: StepFailure,
    },
}

/// A decoded guest instruction, plus what its emitter needs to know.
#[derive(Clone, Debug)]
pub(crate) struct GuestOp {
    /// Where the instruction starts.
    pub address: GuestAddress32,

    /// The decoded instruction.
    pub insn: Insn,

    /// The switch table its `jmp` expands, if any.
    pub switch_table: Option<SwitchTable>,

    /// Whether this is the function's last instruction and also ends a block.
    ///
    /// If it isn't `jmp`, `ret` or `int3`, the analysis probably cut the
    /// function short, so the generated code warns.
    pub last_in_function: bool,

    /// Whether an optimization pass added/changed this instruction.
    ///
    /// If rewritten, the C++ comment reads  `// (rewritten) mov eax, ecx` etc.
    /// so ~~dirty hackers 💀~~ readers know it's not the OG code.
    pub rewritten: bool,

    /// Arithmetic eflags observable after this instruction.
    /// (The flags we need, based on future instructions that read them within func.)
    pub needed_flags: CpuFlag,

    /// Value of DF at this instruction, if known.
    pub direction: DirectionFlag,

    /// x87 condition codes observable after this instruction.
    /// (Same like needed_flags, but for x87 condition codes.)
    pub needed_status: ConditionCodes,

    /// Whether this instruction's memory operand skips `volatile`.
    pub nonvolatile: bool,
}

/// Which way string instructions like `rep movsd` walk memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DirectionFlag {
    /// Check at runtime bozo.
    #[default]
    Unknown,

    /// Addresses go up. (DF clear)
    Forward,

    /// Addresses go down. (DF is set)
    Backward,
}

impl BlockOpIndex {
    /// Wraps a position in a block's ops.
    pub(in crate::codegen) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The position, for indexing [`Block::ops`].
    pub(in crate::codegen) const fn get(self) -> usize {
        self.0
    }
}

impl FunctionModel {
    /// Returns the labels that a jump can reach in this version of the model.
    pub(super) fn local_targets(&self) -> LocalTargets {
        LocalTargets::new(self.blocks.iter().filter_map(|block| block.label))
    }

    /// Whether a pass may change how this function is called.
    ///
    /// Returns `false` if the host replaces the function or hooks into it.
    pub(crate) fn can_change_abi(&self) -> bool {
        self.abi != FunctionAbi::Replaced && self.hooks.is_empty()
    }

    /// Marks the function [`Protected`].
    ///
    /// [`Protected`]: FunctionAbi::Protected
    ///
    /// Call this once a pass changes how the function is called.
    pub(crate) fn protect_abi(&mut self) {
        debug_assert!(self.can_change_abi());
        self.abi = FunctionAbi::Protected;
    }
}

impl Op {
    /// The guest instructions in this operation, in guest (address) order.
    pub(in crate::codegen) fn guests(&self) -> impl Iterator<Item = &GuestOp> {
        let (first, pop, rest): (_, _, &[MergedInstruction]) = match self {
            Op::Guest(guest) => (Some(guest), None, &[]),
            Op::Call(call) => (Some(&call.guest), None, &[]),
            Op::Return(ret) => (Some(&ret.guest), None, &[]),
            Op::CdqIdiv(idiv) => (Some(&idiv.guest), None, &[]),
            Op::MergedCmpJmp(merged) => (
                Some(&merged.compare),
                merged.x87_pop.as_ref(),
                &merged.instructions,
            ),
            Op::Failed { .. } => (None, None, &[]),
        };

        let rest = rest.iter().map(|instruction| &instruction.guest);
        first.into_iter().chain(pop).chain(rest)
    }

    /// Like [`Self::guests`], but each can be changed.
    pub(in crate::codegen) fn guests_mut(&mut self) -> impl Iterator<Item = &mut GuestOp> {
        let (first, pop, rest): (_, _, &mut [MergedInstruction]) = match self {
            Op::Guest(guest) => (Some(guest), None, &mut []),
            Op::Call(call) => (Some(&mut call.guest), None, &mut []),
            Op::Return(ret) => (Some(&mut ret.guest), None, &mut []),
            Op::CdqIdiv(idiv) => (Some(&mut idiv.guest), None, &mut []),
            Op::MergedCmpJmp(merged) => {
                let merged = &mut **merged;
                (
                    Some(&mut merged.compare),
                    merged.x87_pop.as_mut(),
                    &mut merged.instructions,
                )
            }
            Op::Failed { .. } => (None, None, &mut []),
        };

        let rest = rest.iter_mut().map(|instruction| &mut instruction.guest);
        first.into_iter().chain(pop).chain(rest)
    }

    /// Where this operation starts in guest memory.
    pub(in crate::codegen) fn address(&self) -> GuestAddress32 {
        match self {
            Op::Guest(guest) => guest.address,
            Op::Call(call) => call.guest.address,
            Op::Return(ret) => ret.guest.address,
            Op::CdqIdiv(idiv) => idiv.guest.address,
            Op::MergedCmpJmp(merged) => merged.compare.address,
            Op::Failed { address, .. } => *address,
        }
    }

    /// Whether this operation holds the guest instruction at `address`.
    pub(in crate::codegen) fn has_address(&self, address: GuestAddress32) -> bool {
        match self {
            Op::Guest(guest) => guest.address == address,
            Op::Call(call) => call.guest.address == address,
            Op::Return(ret) => ret.guest.address == address,
            Op::CdqIdiv(idiv) => idiv.guest.address == address,
            Op::MergedCmpJmp(merged) => {
                merged.compare.address == address
                    || merged
                        .x87_pop
                        .as_ref()
                        .is_some_and(|pop| pop.address == address)
                    || merged
                        .instructions
                        .iter()
                        .any(|instruction| instruction.guest.address == address)
            }
            Op::Failed { address: at, .. } => *at == address,
        }
    }
}
