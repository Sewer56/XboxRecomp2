//! Picks the memory accesses that can skip `volatile`.
//!
//! See `nonvolatile-memory.md` for more info.

use crate::codegen::function::model::{FunctionModel, GuestOp, Op};
use crate::config::SpeedHacks;
use lock_release::releases_lock_with_mov;
use mark_non_stack_accesses::mark_non_stack_accesses;
use mark_stack_accesses::mark_stack_accesses;
use waiting_reads::WaitingReadFinder;

mod lock_release;
mod mark_non_stack_accesses;
mod mark_stack_accesses;
mod waiting_reads;

pub(super) fn run(models: &mut [FunctionModel], hacks: &SpeedHacks) {
    let mut finder = WaitingReadFinder::new(hacks.runs_plain_sse_memory());
    let mut locks = Vec::new();
    for model in models {
        if hacks.runs_nonvolatile_stack() {
            mark_stack_accesses(model);
        }

        if !hacks.runs_nonvolatile_memory() {
            continue;
        }

        // Lock released with a plain `mov`? Keep the rest `volatile`.
        if releases_lock_with_mov(model, &mut locks) {
            continue;
        }

        // Can't find the loops? Keep the rest `volatile`.
        if !finder.find(model) {
            continue;
        }

        mark_non_stack_accesses(model, &finder.waiting_reads);
    }
}

/// Every guest instruction in `model`, in order.
fn guests(model: &FunctionModel) -> impl Iterator<Item = &GuestOp> {
    let ops = model.blocks.iter().flat_map(|block| &block.ops);
    ops.flat_map(Op::guests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::optimizations::helpers::tests::model_of;
    use core::slice;
    use rstest::rstest;

    /// Runs the pass on `code` and lists the addresses that stay `volatile`.
    fn run_optimisation(code: &[u8], hacks: &SpeedHacks) -> Vec<u32> {
        let mut model = model_of(code);
        run(slice::from_mut(&mut model), hacks);

        // List the volatile addresses.
        guests(&model)
            .filter(|guest| !guest.nonvolatile)
            .map(|guest| guest.address.get())
            .collect()
    }

    #[rstest]
    #[case::same_address(
        &[
                                                //     0x1000:
            0xF7, 0x80, 0x10, 0x04, 0x10, 0x00, //     test dword [eax+0x100410], 0x10000
            0x00, 0x00, 0x01, 0x00,
            0x75, 0xF4,                         //     jnz  0x1000
            0xC3,                               //     ret
        ],
        &[0x1000],
    )]
    #[case::absolute_address(
        &[
                                                //     0x1000:
            0x83, 0x3D, 0x00, 0x00, 0x50, 0x00, //     cmp  dword [0x500000], 0
            0x00,
            0x74, 0xF7,                         //     jz   0x1000
            0xC3,                               //     ret
        ],
        &[0x1000],
    )]
    #[case::call(
        &[
                                                //     0x1000:
            0xE8, 0, 0, 0, 0,                   //     call .next       ; reads no memory
                                                // .next:
            0x85, 0xC0,                         //     test eax, eax
            0x75, 0xF7,                         //     jnz  0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::reloaded_pointer(
        &[
                                                //     0x1000:
            0x8B, 0x44, 0x24, 0x04,             //     mov  eax, [esp+4] ; same value every pass
            0x83, 0x78, 0x10, 0x00,             //     cmp  dword [eax+0x10], 0
            0x74, 0xF6,                         //     jz   0x1000
            0xC3,                               //     ret
        ],
        &[0x1004],
    )]
    #[case::second_way_in(
        &[
            0xE3, 0x01,                         //     jecxz 0x1003     ; jumps into the loop,
                                                //                      ; so everything is kept
                                                //     0x1002:
            0x40,                               //     inc  eax
                                                //     0x1003:
            0x4A,                               //     dec  edx
            0x75, 0xFC,                         //     jnz  0x1002
            0xC3,                               //     ret
        ],
        &[0x1000, 0x1002, 0x1003, 0x1004, 0x1006],
    )]
    #[case::walks_memory(
        &[
                                                //     0x1000:
            0x8B, 0x0E,                         //     mov  ecx, [esi]
            0x83, 0xC6, 0x04,                   //     add  esi, 4      ; next address
            0x4A,                               //     dec  edx
            0x75, 0xF8,                         //     jnz  0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::advancing_pointer(
        &[
                                                //     0x1000:
            0x8B, 0x44, 0x24, 0x04,             //     mov  eax, [esp+4]
            0x83, 0x78, 0x10, 0x00,             //     cmp  dword [eax+0x10], 0
            0x83, 0x44, 0x24, 0x04, 0x04,       //     add  dword [esp+4], 4 ; next address
            0x74, 0xF1,                         //     jz   0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::byte_register_written(
        &[
                                                //     0x1000:
            0x83, 0x38, 0x00,                   //     cmp  dword [eax], 0
            0xB0, 0x01,                         //     mov  al, 1       ; writes part of EAX
            0x74, 0xF9,                         //     jz   0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::stack_only(
        &[
                                                //     0x1000:
            0x83, 0x7C, 0x24, 0x04, 0x00,       //     cmp  dword [esp+4], 0 ; this thread's stack
            0x74, 0xF9,                         //     jz   0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::no_memory(
        &[
                                                //     0x1000:
            0x48,                               //     dec  eax
            0x75, 0xFD,                         //     jnz  0x1000
            0xC3,                               //     ret
        ],
        &[],
    )]
    #[case::stores_before_wait(
        &[
            0x89, 0x4E, 0x08,                   //     mov  [esi+8], ecx   ; the other thread may need it
            0x8B, 0x56, 0x0C,                   //     mov  edx, [esi+0xC] ; not waited on
                                                //     0x1006:
            0x83, 0x3D, 0x00, 0x00, 0x50, 0x00, //     cmp  dword [0x500000], 0
            0x00,
            0x74, 0xF7,                         //     jz   0x1006
            0xC3,                               //     ret
        ],
        &[0x1000, 0x1006],
    )]
    fn run_should_find_volatile_accesses(#[case] code: &[u8], #[case] expected: &[u32]) {
        // Act
        let actual = run_optimisation(code, &SpeedHacks::default());

        // Assert
        assert_eq!(actual, expected);
    }
}
