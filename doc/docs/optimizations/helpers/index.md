# Helpers

!!! info "Building blocks for optimizations that change the calling convention"

Some optimizations break the calling convention (how a function receives
its arguments).  
For example:

- [Stack Argument Elimination] passes parameters in emulated x86 registers.
- [Return Slot Elimination] doesn't put the return address on the stack.

These need the same answers again and again:

- Where does ESP point at each instruction?
- Which argument slots can we recognize?
- Which arguments get written to the stack before a call?
- Can we find and rewrite every caller?

The helpers I made answer those, so each optimization doesn't have to.

!!! note "Where this happens"

    `codegen/optimizations/helpers/`, under `src/xbox-recomp2/src/`.

## The helpers

| Helper            | Answers                                                  |
| ----------------- | -------------------------------------------------------- |
| [Stack Tracking]  | Where do ESP and EBP point before each instruction?      |
| [Function Stacks] | How does each call move ESP?                             |
| [Stack Arguments] | Which slots are recognized, and who removes them?        |
| [Entry Loads]     | Which arguments get read off the stack at the start?     |
| [Call Setup]      | Which arguments get writte n to the stack before a call? |
| [Known Callers]   | Can we find and rewrite every caller?                    |
| [Stack Shift]     | Move every stack access above some point by N bytes.     |
| [Flag Effects]    | Which EFLAGS does one instruction read and write?        |
| [Flag Boundaries] | Which EFLAGS cross each call and `ret`?                  |
| [Needed Flags]    | Which EFLAGS must be valid after each operation?         |

## Remarks

Should be obvious, but these helpers aren't updated in real time, if you
change the code from a pass; you gotta recompute.

[Stack Argument Elimination]: ../stack-argument-elimination.md
[Return Slot Elimination]: ../return-slot-elimination.md
[Stack Tracking]: stack-tracking.md
[Function Stacks]: function-stacks.md
[Stack Arguments]: stack-arguments.md
[Entry Loads]: entry-loads.md
[Call Setup]: call-setup.md
[Known Callers]: known-callers.md
[Stack Shift]: stack-shift.md
[Flag Effects]: flag-effects.md
[Flag Boundaries]: flag-boundaries.md
[Needed Flags]: needed-flags.md
