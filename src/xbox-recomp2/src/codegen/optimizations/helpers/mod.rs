//! Shared analysis for optimization passes.
//!
//! Most of these help passes that change how functions get called.
//!
//! Tracking the stack:
//!
//! - [`function_stacks`]: every function's stack, so we can follow calls.
//! - [`function_stack`]: where ESP and EBP point before each instruction.
//! - [`stack_frame`]: steps ESP and EBP through one instruction.
//!
//! Finding arguments:
//!
//! - [`stack_arguments`]: how many arguments a function takes, and who pops them.
//! - [`entry_loads`]: finds arguments read off the stack when a function starts.
//! - [`call_setup`]: finds arguments written to the stack before a call.
//!
//! Rewriting:
//!
//! - [`known_callers`]: drops functions with a caller we can't find or rewrite.
//! - [`stack_shift`]: moves stack accesses above a boundary by a fixed amount.
//!
//! Tracking `eflags`:
//!
//! - [`flag_boundaries`]: which `eflags` cross each call and `ret` in every function.
//! - [`flag_summary`]: what running a function does to its caller's `eflags`.
//! - [`needed_flags`]: which `eflags` may still be read after each instruction.
//!
//! Looping until nothing changes:
//!
//! - [`workqueue`]: the queue of items still to visit.
//!
//! Naming functions:
//!
//! - [`model_index`]: a function's position in the model list.
//!
//! Walking one function's ops:
//!
//! - [`op_graph`]: its ops, where each can go next, and where its hooks go.
//! - [`loops`]: the loops in an [`op_graph`].
//!
//! Single instructions, used by the helpers above:
//!
//! - [`instruction`]: which registers an instruction touches, etc.

pub(in crate::codegen) mod call_setup;
pub(in crate::codegen) mod entry_loads;
pub(in crate::codegen) mod flag_boundaries;
pub(in crate::codegen) mod flag_summary;
pub(in crate::codegen) mod function_stack;
pub(in crate::codegen) mod function_stacks;
pub(in crate::codegen) mod instruction;
pub(in crate::codegen) mod known_callers;
pub(in crate::codegen) mod loops;
pub(in crate::codegen) mod model_index;
pub(in crate::codegen) mod needed_flags;
pub(in crate::codegen) mod op_graph;
pub(in crate::codegen) mod stack_arguments;
pub(in crate::codegen) mod stack_frame;
pub(in crate::codegen) mod stack_shift;
pub(in crate::codegen) mod workqueue;
#[cfg(test)]
pub(in crate::codegen) mod tests {
    use crate::analysis::{Function, FunctionGraph, discover};
    use crate::codegen::function::model::FunctionModel;
    use crate::codegen::function::{FunctionScratch, build_function};
    use crate::config::{FunctionAddress, RecompilerConfig};
    use crate::decode::{DecodeBuffer, DecodeOutcome, Decoder, Insn, Mnemonic, Register};
    use crate::executable::{Executable, ImportMap, Section, SectionFlags};
    use crate::platform::{GuestPlatform, MemoryRoute};

    /// Base address of the fixture's section, entry point and function.
    const BASE: u64 = 0x1000;

    /// Decodes one instruction from `code`.
    pub(in crate::codegen) fn instruction(code: &[u8]) -> Insn {
        let mut buffer = DecodeBuffer::new();
        let DecodeOutcome::Ok(insn) = buffer.decode_single_instruction(&Decoder::new(), code)
        else {
            panic!("invalid test instruction")
        };
        insn.clone()
    }

    /// Runs discovery on `code`, then builds the function found at [`BASE`].
    pub(in crate::codegen) fn analyzed_function(code: &[u8]) -> FunctionModel {
        let (_, models) = program(&mut executable(code), &RecompilerConfig::default());
        models
            .into_iter()
            .find(|model| u64::from(model.address.0) == BASE)
            .unwrap()
    }

    /// Builds a function from all of `code`, placed at [`BASE`].
    pub(in crate::codegen) fn model_of(code: &[u8]) -> FunctionModel {
        build_function(
            &executable(code),
            &RecompilerConfig::default(),
            &Decoder::new(),
            &Function {
                base: BASE.into(),
                size: code.len() as u64,
            },
            None,
            &mut FunctionScratch::new(),
            &mut Vec::new(),
        )
    }

    /// A platform that treats all memory as plain RAM and never redirects calls.
    pub(in crate::codegen) struct PlainPlatform;

    impl GuestPlatform for PlainPlatform {
        fn route(
            &self,
            _: &ImportMap,
            _: Mnemonic,
            _: Register,
            _: u32,
            _: u16,
            _: bool,
        ) -> MemoryRoute {
            MemoryRoute::Plain
        }

        fn register_call_window(&self) -> Option<(u32, u32)> {
            None
        }
    }

    /// An executable with `code` as its `.text` at [`BASE`].
    pub(in crate::codegen) fn executable(code: &[u8]) -> Executable {
        Executable {
            data: code.to_vec(),
            size: code.len() as u32,
            base: BASE.into(),
            entry_point: BASE.into(),
            sections: vec![Section::new(
                ".text",
                BASE,
                code.len() as u32,
                SectionFlags::CODE,
                0,
            )],
            ..Executable::default()
        }
    }

    /// Places each `(address, code)` from [`BASE`], with `int3` in between.
    pub(in crate::codegen) fn layout(parts: &[(u32, &[u8])]) -> Vec<u8> {
        let mut code = Vec::new();
        for &(address, part) in parts {
            code.resize((u64::from(address) - BASE) as usize, 0xCC);
            code.extend_from_slice(part);
        }
        code
    }

    /// Discovers `executable` and builds a model for each function.
    pub(in crate::codegen) fn program(
        executable: &mut Executable,
        config: &RecompilerConfig,
    ) -> (FunctionGraph, Vec<FunctionModel>) {
        let decoder = Decoder::new();
        let mut config = config.clone();
        let discovered = discover(executable, &mut config, &decoder, &mut Vec::new());
        let mut scratch = FunctionScratch::new();
        let models = discovered
            .functions
            .iter()
            .map(|function| {
                build_function(
                    executable,
                    &config,
                    &decoder,
                    function,
                    discovered.bodies.get(&function.base),
                    &mut scratch,
                    &mut Vec::new(),
                )
            })
            .collect();
        (discovered.graph, models)
    }

    /// The function at `address`.
    pub(in crate::codegen) fn model_at(models: &[FunctionModel], address: u32) -> &FunctionModel {
        models
            .iter()
            .find(|model| model.address == FunctionAddress(address))
            .unwrap()
    }
}
