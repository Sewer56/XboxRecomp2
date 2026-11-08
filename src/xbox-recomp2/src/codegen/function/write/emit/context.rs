//! All the context you need to turn one instruction into C++.

use super::{CodeWriter, operands};
use crate::analysis::LocalTargets;
use crate::codegen::function::model::ALL_ARITHMETIC_FLAGS;
use crate::config::RecompilerConfig;
use crate::decode::{CpuFlag, DecodedOperand, Insn};
use crate::executable::{ImportMap, SwitchTable, SymbolTable};
use crate::platform::{GuestPlatform, MemoryRoute};
use xbox_recomp2_executable::GuestAddress32;
use zydis::Register;

/// All the context you need to turn one instruction into C++.
pub(in crate::codegen::function::write) struct Emitter<'a> {
    /// Where you write the C++.
    pub out: &'a mut CodeWriter,

    /// Where this instruction is in the original exe/xbe.
    pub base: GuestAddress32,

    /// The instruction you're turning into C++.
    pub insn: &'a Insn,

    /// eflags observable after this instruction.
    pub needed_eflags: CpuFlag,

    /// The program's functions.
    pub symbols: &'a SymbolTable,

    /// OS functions the program calls.
    pub imports: &'a ImportMap,

    /// Settings for the whole run.
    pub config: &'a RecompilerConfig,

    /// The platform you're targeting, e.g. Xbox or Windows.
    pub platform: &'a dyn GuestPlatform,

    /// Notes shared between instructions in this function.
    pub function_state: &'a mut FunctionState,

    /// The jump table for this `jmp`, if there is one.
    pub switch_table: Option<&'a SwitchTable>,

    /// Set by [`Self::missing_form`].
    pub form_missing: bool,
}

/// State the emitter keeps while writing one function.
///
/// Most instructions don't depend on each other.
/// This holds the few things one instruction needs to pass to a later one.
///
/// # Example
///
/// ```asm
/// mov eax, [0x1000]   ; 0x1000 holds an OS function's address
/// call eax
/// ```
///
/// The `mov` sets [`has_register_import`] and [`register_import_thunk`].
/// The `call` reads them to call that OS function.
///
/// [`has_register_import`]: Self::has_register_import
/// [`register_import_thunk`]: Self::register_import_thunk
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::codegen::function::write) struct FunctionState {
    /// Whether an earlier `mov` put an OS function's address in a register.
    pub has_register_import: bool,

    /// Where that `mov` read the address from, such as `0x1000`.
    pub register_import_thunk: u32,

    /// Where the code blocks start.
    /// `jmp`(s) use `goto` to go there.
    pub local_targets: LocalTargets,

    /// Whether jmp(s) with a 16-bit operand keep only the target's low 16 bits.
    pub mask_branch_targets: bool,

    /// Whether x87 code works on the local copy `fpu` instead of `ctx.fpu`.
    pub local_fpu: bool,

    /// Whether MMX code uses the local array `mmx` instead of `ctx.fpu.mmx`.
    pub local_mmx: bool,

    /// Whether `[ebp+N]` always points into this function's stack frame.
    pub ebp_frame: bool,
}

impl FunctionState {
    /// The C++ name of the x87 state: `fpu` or `ctx.fpu`.
    pub fn fpu(&self) -> &'static str {
        if self.local_fpu { "fpu" } else { "ctx.fpu" }
    }

    /// The C++ name of the MMX registers: `mmx` or `ctx.fpu.mmx`.
    pub fn mmx(&self) -> &'static str {
        if self.local_mmx { "mmx" } else { "ctx.fpu.mmx" }
    }
}

impl Emitter<'_> {
    /// The C++ name of the x87 state, like:
    /// - `ctx.fpu`
    /// - `fpu`
    ///
    /// Which depends on `local_fpu` optimisation.
    /// Write x87 code as `{fpu}.st0()`.
    pub fn fpu(&self) -> &'static str {
        self.function_state.fpu()
    }

    /// The C++ name of the MMX register array:
    /// - `ctx.fpu.mmx`
    /// - `mmx`
    ///
    /// Which depends on `local_mmx` optimisation.
    /// Write MMX registers as `{mmx}[3]`.
    pub fn mmx(&self) -> &'static str {
        self.function_state.mmx()
    }

    /// The C++ text for one MMX register, like `MMX_REG(3)` for `mm3`.
    ///
    /// # Arguments
    ///
    /// - `index`: register number, like `3` for `mm3`.
    ///
    /// # Returns
    ///
    /// `MMX_REG(3)`, or `mmx[3]` when MMX code uses the local array.
    pub fn mmx_reg(&self, index: i64) -> String {
        if self.function_state.local_mmx {
            format!("{}[{index}]", self.mmx())
        } else {
            format!("MMX_REG({index})")
        }
    }

    /// Builds the template arguments for a helper that sets flags.
    ///
    /// # Arguments
    ///
    /// - `memory`: the helper takes `Volatile` first.
    ///
    /// # Returns
    ///
    /// `<0xMASK>`, or `<true, 0xMASK>` when `memory` is true.
    pub fn flag_template(&self, memory: bool) -> String {
        if self.needed_eflags == ALL_ARITHMETIC_FLAGS {
            return String::new();
        }
        let mask = self.needed_eflags.bits();
        if memory {
            format!("<true, 0x{mask:X}>")
        } else {
            format!("<0x{mask:X}>")
        }
    }

    /// Returns a memory operand's address.
    ///
    /// See [`operands::ea`] for more info.
    pub fn ea(&self, operand: &DecodedOperand) -> String {
        operands::ea(operand, self.native_addresses())
    }

    /// Returns an operand as a C++ expression.
    ///
    /// See [`operands::format_operand`] for more info.
    pub fn format_operand(&self, operand: &DecodedOperand, size_suffix: &str) -> String {
        operands::format_operand(operand, size_suffix, self.native_addresses())
    }

    /// Does this guest address use host's native word size?
    pub fn native_addresses(&self) -> bool {
        self.config.speed_hacks.runs_native_sized_addresses()
    }

    /// Marks this operand form as unsupported, so the writer reports it as missing.
    pub fn missing_form(&mut self) {
        self.form_missing = true;
    }

    /// Returns whether a jump to `target` stays inside the current function.
    ///
    /// If it does, you can write the jump as a `goto`.
    pub fn is_local_target(&self, target: GuestAddress32) -> bool {
        self.function_state.local_targets.contains(target)
    }

    /// Cuts a jump address down to 16 bits if this is a 16-bit jump.
    ///
    /// For example, `0x12345` becomes `0x2345`.
    pub fn masked_branch_target(&self, target: GuestAddress32) -> GuestAddress32 {
        if self.function_state.mask_branch_targets && self.insn.instruction().operand_width == 16 {
            GuestAddress32::from(target.get() & 0xffff)
        } else {
            target
        }
    }

    /// Asks the platform what C++ to write for a memory read or write.
    ///
    /// `size` is in bits. See [`crate::platform`] for an example.
    pub fn route(&self, segment: Register, address: u32, size: u16, is_write: bool) -> MemoryRoute {
        self.platform.route(
            self.imports,
            self.insn.mnemonic(),
            segment,
            address,
            size,
            is_write,
        )
    }

    /// Writes the lines from a [`MemoryRoute::Statement`].
    ///
    /// Replaces each `{reg}` in `template` with `reg`.
    pub(crate) fn emit_platform_statement(&mut self, template: &str, reg: &str) {
        for line in template.replace("{reg}", reg).lines() {
            self.out.raw(line).raw("\n");
        }
    }
}
