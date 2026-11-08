//! Passes stack arguments in registers, where it's safe.
//!
//! See `eliminate-stack-args.md` for more info.

use crate::analysis::FunctionGraph;
use crate::codegen::function::model::FunctionModel;
use crate::codegen::optimizations::helpers::function_stacks::FunctionStacks;
use crate::codegen::optimizations::helpers::known_callers;
use called::Called;
use std::collections::BTreeMap;
use std::io::Write;

mod called;
mod caller;

pub(in crate::codegen) fn run(
    models: &mut [FunctionModel],
    graph: &FunctionGraph,
    trust_callee_returns: bool,
    diag: &mut dyn Write,
) {
    // Find the functions that can take arguments in registers.
    // TODO: Only rebuild entries for functions a pass changed, for faster builds.
    let stacks = FunctionStacks::build(models, trust_callee_returns);
    let mut called: BTreeMap<_, _> = models
        .iter()
        .filter_map(|model| {
            let stack = stacks.get(model.address)?;
            Some((model.address, Called::find(model, stack)?))
        })
        .collect();

    // Keep only the ones which are safe to rewrite.
    known_callers::retain(&mut called, models, graph, |function, ops, call| {
        function.call_setup(&ops[..call]).is_some()
    });
    drop(stacks);

    // Rewrite the calls, and the callers.
    let mut rewritten = caller::Rewritten::default();
    for model in models.iter_mut().filter(|model| model.can_change_abi()) {
        caller::rewrite_calls(model, &called, &mut rewritten);
        if let Some(function) = called.get(&model.address) {
            function.rewrite(model);
        }
    }

    let caller::Rewritten { calls, pushes } = rewritten;
    let _ = writeln!(
        diag,
        "eliminate-stack-args: rewrote {calls} calls ({} callees), removed {pushes} pushes",
        called.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::function::model::{FunctionAbi, Op};
    use crate::codegen::optimizations::helpers::tests::{executable, layout, model_at, program};
    use crate::config::RecompilerConfig;
    use crate::decode::{DISASM_BUFFER_SIZE, DisasmFormatter, Register};
    use rstest::rstest;

    #[rstest]
    #[case::caller_cleans_up(
        &[
            0x52,                   // push edx             ; argument 0
            0xE8, 0x3A, 0, 0, 0,    // call 0x1040
            0x83, 0xC4, 4,          // add  esp, 4
            0xC3,                   // ret
        ],
        &[],
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; entry load
            0x89, 0x01,             // mov  [ecx], eax
            0xC3,                   // ret
        ],
        &["push edx", "mov eax, edx", "call 4", "add esp, 0x04", "ret"], // <- push stays
        &["mov [ecx], eax", "ret"],
    )]
    #[case::called_cleans_up(
        &[
            0x52,                   // push edx             ; argument 0
            0xE8, 0x3A, 0, 0, 0,    // call 0x1040
            0xC3,                   // ret
        ],
        &[],
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; entry load
            0x89, 0x01,             // mov  [ecx], eax
            0xC2, 4, 0,             // ret  4
        ],
        &["mov eax, edx", "call 8", "ret"], // <- push gone, the call moves ESP for it
        &["mov [ecx], eax", "ret 0x04"],
    )]
    #[case::two_calls(
        &[
            0x52,                   // push edx             ; call 1: stays
            0xBA, 1, 0, 0, 0,       // mov  edx, 1          ; <- edx no longer holds it
            0xE8, 0x35, 0, 0, 0,    // call 0x1040          ; call 1: one op added
            0x50,                   // push eax             ; call 2: already in eax
            0xE8, 0x2F, 0, 0, 0,    // call 0x1040          ; call 2: one op removed
            0xC3,                   // ret
        ],
        &[],
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; entry load
            0x89, 0x01,             // mov  [ecx], eax
            0xC2, 4, 0,             // ret  4
        ],
        &["push edx", "mov edx, 0x01", "mov eax, [esp]", "call 4", "call 8", "ret"],
        &["mov [ecx], eax", "ret 0x04"],
    )]
    #[case::caller_without_setup(
        &[
            0x52,                   // push edx
            0xE8, 0x3A, 0, 0, 0,    // call 0x1040
            0xC3,                   // ret
        ],
        &[
            0xE8, 0x1B, 0, 0, 0,    // call 0x1040          ; <- no argument write to find
            0xC3,                   // ret
        ],
        &[
            0x8B, 0x44, 0x24, 4,    // mov  eax, [esp+4]    ; entry load
            0x89, 0x01,             // mov  [ecx], eax
            0xC2, 4, 0,             // ret  4
        ],
        &["push edx", "call 4", "ret"], // <- all callers or none: unchanged
        &["mov eax, [esp+0x04]", "mov [ecx], eax", "ret 0x04"],
    )]
    fn run_should_rewrite_caller_and_called_function(
        #[case] caller: &[u8],
        #[case] other_caller: &[u8],
        #[case] called: &[u8],
        #[case] expected_caller: &[&str],
        #[case] expected_called: &[&str],
    ) {
        // Arrange
        let code = layout(&[(0x1000, caller), (0x1020, other_caller), (0x1040, called)]);
        let (graph, mut models) = program(&mut executable(&code), &RecompilerConfig::default());

        // Act
        run(&mut models, &graph, false, &mut Vec::new());

        // Assert
        assert_eq!(
            listing(&model_at(&models, 0x1000).blocks[0].ops),
            expected_caller
        );

        let called = model_at(&models, 0x1040);
        let rewritten = called.abi == FunctionAbi::Protected;
        let arguments: &[_] = if rewritten {
            &[(0, Register::EAX)]
        } else {
            &[]
        };
        assert_eq!(called.fastcall.register_arguments, arguments);
        assert_eq!(listing(&called.blocks[0].ops), expected_called);
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Disassembles `ops`.
    pub(super) fn listing(ops: &[Op]) -> Vec<String> {
        let formatter = DisasmFormatter::new();
        let mut text = [0; DISASM_BUFFER_SIZE];
        ops.iter()
            .map(|op| match op {
                Op::Guest(guest) => formatter
                    .format(&guest.insn, 0, &mut text)
                    .unwrap()
                    .to_owned(),
                Op::CdqIdiv(_) => panic!("idiv32_after_cdq runs after this pass"),
                Op::Call(call) => format!("call {}", call.stack_bytes),
                Op::Return(ret) => formatter
                    .format(&ret.guest.insn, 0, &mut text)
                    .unwrap()
                    .to_owned(),
                Op::MergedCmpJmp(_) => panic!("flag passes run after this pass"),
                Op::Failed { .. } => panic!("unexpected decode failure"),
            })
            .collect()
    }
}
