//! Skips writing arithmetic flags that nothing reads.
//!
//! See `skip-unused-flags.md` for more info.

use super::helpers::flag_boundaries::FlagBoundaries;
use super::helpers::instruction::flag_effects::EFlagEffects;
use super::helpers::needed_flags::NeededFlags;
use crate::codegen::function::model::{FunctionModel, Op};

pub(super) fn run(models: &mut [FunctionModel], boundaries: &FlagBoundaries) {
    for model in models {
        narrow_function(model, boundaries);
    }
}

fn narrow_function(model: &mut FunctionModel, boundaries: &FlagBoundaries) {
    // Find out which flags are read after each instruction.
    let live_after = match NeededFlags::new(model, boundaries) {
        Some(live) => live.live_after,
        None => return,
    };

    // And now set a limited amount of flags, if possible. Thanks!
    let ops = model.blocks.iter_mut().flat_map(|block| &mut block.ops);
    for (op, needed) in ops.zip(live_after) {
        match op {
            Op::Guest(guest) if EFlagEffects::can_narrow(&guest.insn) => {
                guest.needed_flags = needed;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::function::model::{ALL_ARITHMETIC_FLAGS, Hook};
    use crate::codegen::optimizations::helpers::tests::model_of;
    use crate::codegen::optimizations::merge_cmp_jmp;
    use crate::decode::CpuFlag;
    use core::slice;
    use rstest::rstest;

    const ALL: CpuFlag = ALL_ARITHMETIC_FLAGS;
    const NONE: CpuFlag = CpuFlag::empty();

    /// What to do before running the pass.
    enum Setup {
        /// Nothing.
        Plain,

        /// Run `merge_cmp_jmp` first.
        Merge,

        /// Add a hook at this address.
        Hook(u32),
    }

    /// The flags needed after each instruction, in order.
    fn needed_flags(model: &FunctionModel) -> Vec<CpuFlag> {
        model
            .blocks
            .iter()
            .flat_map(|block| &block.ops)
            .filter_map(|op| match op {
                Op::Guest(guest) => Some(guest.needed_flags),
                _ => None,
            })
            .collect()
    }

    #[rstest]
    #[case::overwrite(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x39, 0xC8,             //     cmp  eax, ecx   ; overwrites every flag
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[NONE],
    )]
    #[case::carry(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x11, 0xC8,             //     adc  eax, ecx   ; reads CF
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[CpuFlag::CF],
    )]
    #[case::inc(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x41,                   //     inc  ecx        ; keeps CF
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[CpuFlag::CF],
    )]
    #[case::lahf(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x9F,                   //     lahf            ; reads every flag but OF
            0x39, 0xC8,             //     cmp  eax, ecx
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL & !CpuFlag::OF],
    )]
    #[case::pushfd(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x9C,                   //     pushfd          ; reads every flag
            0x59,                   //     pop  ecx
            0x39, 0xC8,             //     cmp  eax, ecx
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::variable_shift(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0xD3, 0xE0,             //     shl  eax, cl    ; cl may be 0: keeps every flag
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::nonzero_shift(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0xD1, 0xE0,             //     shl  eax, 1     ; overwrites every flag
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[NONE],
    )]
    #[case::ret(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0xC3,                   //     ret             ; caller sees the flags
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::call(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0xFF, 0xD1,             //     call ecx        ; callee sees the flags
            0x39, 0xC8,             //     cmp  eax, ecx
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::external_jump(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0xFF, 0xE1,             //     jmp  ecx        ; unknown target sees the flags
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::external_direct(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x74, 0x10,             //     jz   0x1014     ; outside the function: sees the flags
            0x39, 0xC8,             //     cmp  eax, ecx
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::one_live_path(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x74, 2,                //     jz   .ret       ; taken: caller sees the flags
            0x39, 0xC8,             //     cmp  eax, ecx
                                    // .ret:
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[ALL],
    )]
    #[case::loop_carry(
        &[
            0x01, 0xD8,             //     add  eax, ebx
                                    // .loop:
            0x41,                   //     inc  ecx        ; keeps CF
            0x75, 0xFD,             //     jnz  .loop
            0x0F, 0x92, 0xC2,       //     setb dl         ; reads CF from `add`
            0x39, 0xD8,             //     cmp  eax, ebx
            0xC3,                   //     ret
        ],
        Setup::Plain,
        &[CpuFlag::CF, CpuFlag::CF | CpuFlag::ZF],
    )]
    #[case::merged_jump(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x83, 0xF8, 0x20,       //     cmp  eax, 32    ; merged: jz reads eax, not flags
            0x74, 0,                //     jz   .next
                                    // .next:
            0x39, 0xD1,             //     cmp  ecx, edx   ; both paths overwrite every flag
            0xC3,                   //     ret
        ],
        Setup::Merge,
        &[NONE, ALL],
    )]
    #[case::hook(
        &[
            0x01, 0xD8,             //     add  eax, ebx
            0x39, 0xD1,             //     cmp  ecx, edx   ; hooked: hook sees the flags
            0xC3,                   //     ret
        ],
        Setup::Hook(0x1002),
        &[ALL],
    )]
    fn run_should_keep_only_read_flags(
        #[case] code: &[u8],
        #[case] setup: Setup,
        #[case] expected: &[CpuFlag],
    ) {
        // Arrange
        let mut model = model_of(code);
        let boundaries = FlagBoundaries::default();
        match setup {
            Setup::Plain => {}
            Setup::Merge => merge_cmp_jmp::run(slice::from_mut(&mut model), &boundaries),
            Setup::Hook(address) => model.hooks.push(Hook {
                address: address.into(),
            }),
        }

        // Act
        run(slice::from_mut(&mut model), &boundaries);

        // Assert
        assert_eq!(&needed_flags(&model)[..expected.len()], expected);
    }
}
