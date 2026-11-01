# Optimizations

!!! info "We do some post-processing of decoded ASM code to make final code faster"

Optimizations run after a function is turned into the [function model],
and before being written out as C++.

## The optimizations

| Optimization              | What it does                                        | Status        |
| ------------------------- | --------------------------------------------------- | ------------- |
| [`eliminate_return_slot`] | Doesn't put return addresses on the stack.          | Working       |
| [`eliminate_stack_args`]  | Optimizes away push+pop across function calls.      | Working       |
| [`x87_lowering`]          | Keeps x87 values in local variables.                | Not built yet |
| [`fuse_compare_branch`]   | Turns cmp + jump into a plain `if (a < b)`.         | Working       |
| [`eliminate_dead_flags`]  | Only computes arithmetic flags that are read later. | Working       |

## Helpers

Optimizations that change how functions are called share some [helpers].<br/>
e.g. to track the stack, or find every caller of a function.

## How they run

The recompiler builds every function first.<br/>
Then it runs the optimizations in two groups:

1. **Whole-program** optimizations change how functions call each other.
2. **Per-function** optimizations look at one function at a time.

| Optimization              | Group         |
| ------------------------- | ------------- |
| [`eliminate_return_slot`] | Whole-program |
| [`eliminate_stack_args`]  | Whole-program |
| [`fuse_compare_branch`]   | Whole-program |
| [`eliminate_dead_flags`]  | Whole-program |
| [`x87_lowering`]          | Per-function  |

[function model]: ../recompiling-functions.md
[`eliminate_return_slot`]: return-slot-elimination.md
[`eliminate_stack_args`]: stack-argument-elimination.md
[`x87_lowering`]: x87-lowering.md
[`eliminate_dead_flags`]: skip-unused-flags.md
[`fuse_compare_branch`]: merge-compare-and-jump.md
[helpers]: helpers/index.md
