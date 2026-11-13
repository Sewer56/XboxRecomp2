# Optimizations

!!! info "We do some post-processing of decoded ASM code to make final code faster"

Optimizations run after a function is turned into the [function model],
and before being written out as C++.

## The optimizations

| Optimization                  | What it does                                                     |
| ----------------------------- | ---------------------------------------------------------------- |
| [`eliminate_return_slot`]     | Doesn't put return addresses on the stack.                       |
| [`eliminate_stack_args`]      | Optimizes away push+pop across function calls.                   |
| [`optimize_x87`]              | Works out the x87 stack depth, so x87 code can use a local copy. |
| [`optimize_mmx`]              | Keeps MMX registers in a local array.                            |
| [`eliminate_dead_x87_status`] | Skips x87 C0-C3 status-bit writes nothing reads.                 |
| [`fuse_compare_branch`]       | Turns cmp + jump into a plain `if (a < b)`.                      |
| [`eliminate_dead_flags`]      | Only computes arithmetic flags that are read later.              |
| [`find_ebp_frames`]           | Finds functions that use EBP as the frame pointer.               |
| [`idiv32_after_cdq`]          | Uses a 32-bit `idiv` after `cdq`.                                |
| [`find_string_direction`]     | Skips the direction check in `rep movs` and co.                  |
| [`nonvolatile_memory`]        | Speed hack. Lets Clang merge or drop guest loads and stores.     |
| [`stack_slots_in_locals`]     | Speed hack. Keeps leaf functions' stack slots in a local array.  |

## Framerate

!!! info "How much each setting speeds up Shadow the Hedgehog."

Each setting is turned on alone, everything else off.<br/>
Stage `0201`, 20 seconds of idle at spawn, 8 runs per setting, taking turns.<br/>
Game pinned to the CPUs sharing the big L3 cache.

Base (nothing on) runs at about 1,719 FPS.

| Setting                     | vs base |     ± |
| --------------------------- | ------: | ----: |
| All on                      | +12.50% | 1.26% |
| `optimize_x87`              |  +3.79% | 1.04% |
| `eliminate_dead_flags`      |  +3.01% | 0.54% |
| `fast_sse_status`           |  +1.63% | 0.64% |
| `fast_x87_stack`            |  +1.50% | 1.13% |
| `fuse_compare_branch`       |  +1.21% | 0.81% |
| `rep_memmove_memset`        |  +1.06% | 0.89% |
| `fast_x87_status`           |  +0.96% | 1.13% |
| `native_sized_addresses`    |  +0.50% | 1.36% |
| `idiv32_after_cdq`          |  +0.47% | 0.52% |
| `nonvolatile_stack`         |  +0.31% | 0.87% |
| `find_string_direction`     |  +0.29% | 0.77% |
| `eliminate_stack_args`      |  +0.02% | 0.78% |
| `plain_sse_memory`          |  −0.18% | 0.58% |
| `eliminate_return_slot`     |  −0.19% | 1.54% |
| `fast_fscale`               |  −0.21% | 0.78% |
| `fast_trig`                 |  −0.40% | 1.72% |
| `lazy_cmov_load`            |  −0.64% | 0.67% |
| `optimize_mmx`              |  −0.77% | 0.71% |
| `eliminate_dead_x87_status` |  −1.17% | 1.95% |

!!! warning "Wait- some of them made the game slower?"

    Nope. They're just micro-optimisations, too small to notice!<br/>
    Much more likely stuff like CPU clock speed and background loads pop up here!<br/>
    (This is where looking at the assembly helps! You know when it *should* be faster.)

    If you compare the sums of `All On`, vs running game with all of the ones 
    below 0%, you'll notice 'All On' is faster. By >1%.

    Also test was kind of flawed, I used a spawn area, difference is bigger when
    there's enemies/objects around; just hard to deterministic measure that.

## Helpers

Optimizations that change how functions are called share some [helpers].<br/>
e.g. to track the stack, or find every caller of a function.

[function model]: ../recompiling-functions.md
[`eliminate_return_slot`]: return-slot-elimination.md
[`eliminate_stack_args`]: stack-argument-elimination.md
[`optimize_x87`]: x87.md
[`optimize_mmx`]: mmx-locals.md
[`eliminate_dead_x87_status`]: skip-unused-x87-status.md
[`eliminate_dead_flags`]: skip-unused-flags.md
[`fuse_compare_branch`]: merge-compare-and-jump.md
[`find_ebp_frames`]: find-ebp-frames.md
[`idiv32_after_cdq`]: idiv-after-cdq.md
[`find_string_direction`]: find-string-direction.md
[`nonvolatile_memory`]: nonvolatile-memory.md
[`stack_slots_in_locals`]: stack-slots-in-locals.md
[helpers]: helpers/index.md
