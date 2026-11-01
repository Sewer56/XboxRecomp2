# Flag Boundaries

Works out which arithmetic flags cross calls and returns, for every function.

!!! note "Where this happens"

    `FlagBoundaries::build` in `codegen/optimizations/helpers/flag_boundaries/`.

## The problem

Every recompiled function shares one `X86Context&`, flags included:

```cpp
void sub_A(X86Context& ctx, uint8_t* base); // <- ctx.eflags too
```

In *really, really, really rare circumstances*, a caller can read flags after a 
`call`.

For example, `pow` in Shadow's C runtime (CRT):

```asm
; Loads the double at [edx] onto the x87 stack.
; Sets ZF if it's infinity or NaN.
sub_3824F9:
    mov  eax, [edx+4]
    and  eax, 0x7FF00000
    cmp  eax, 0x7FF00000
    jz   inf_or_nan
    fld  qword ptr [edx]
    ret                     ; <- ZF = 0
inf_or_nan:
    ...                     ; loads it another way
    test eax, 0
    ret                     ; <- ZF = 1

; pow(x, y)
sub_37DF88:
    ...
    call sub_3824F9
    jz   x_is_inf_or_nan    ; <- reads ZF from sub_3824F9
```

However in 99% of cases, `eflags` are not preserved after a call.<br/>
They are clobbered/discarded, or simply not needed.

But, computing/emulating these flags at runtime is... expensive.

!!! tip "We need to do it safely!"

## How it works

By default, [Needed Flags] handles one function at a time.<br/>
However, there is some context it lacks.

- What each function it calls does with flags. (i.e. what happens across a `call`)
- Which flags its callers read after its `ret`. (i.e. what we can skip computing)

We run [Needed Flags] on every function once.  
Then we rerun only the functions whose inputs changed, until nothing changes.

### 1. What each function does with flags

We call this a 'summary':

```rust
struct FlagSummary {
    reads: CpuFlag, // flags it reads before writing them
    keeps: CpuFlag, // flags that reach its `ret` unmodified
}
```

For example, `mov eax, ebx; ret` keeps every flag, and `cmp eax, ebx; ret`
keeps none.

When a function's summary changes, we recheck its callers.
Summaries start empty and only grow.

!!! info "This baka-chan called 'recursion' can't hurt you"

### 2. What callers read after each `ret`

We store this as `at_return`, one entry per function:

```rust
struct FlagBoundaries {
    summaries: Vec<FlagSummary>, // from step 1, each caller's read flags
    at_return: Vec<CpuFlag>,     // flags callers can read after its `ret`
}
```

We fill it in from each caller.
[FlagGraph::walk] gives the caller's
`live_after`: the flags still needed after each instruction.

```asm
caller:
    call sub_A   ; jz reads ZF, so at_return[sub_A] gets ZF
    jz   done
    jmp  sub_B   ; tail jump: at_return[sub_B] gets at_return[caller]
done:
    ret
```

A tail jump's target returns straight to our caller.
That's why `sub_B` gets the caller's own `at_return`.

When a function's `at_return` grows, we recheck it.

## When we can't see the code

!!! info "When we can't see the code"

    When it can't, we assume that code needs every flag.

### Code we can't see (step 1)

!!! info "If we can't see some code, we assume it keeps every flag."

We use `FlagSummary::BARRIER` for this.<br/>
Examples include:

- Indirect call/jmp e.g. `call [eax]`: we don't know the target.
- Host replacements (`function_hooks`).
- A direct jump into the middle of another function.

### Callers we can't see (step 2)

!!! info "If we can't see every caller, we assume they need every flag."

Examples include:

- The entry point.
- Callbacks and vtable entries. (We can't analyze them 100%, only guess)

## Speed hack: `clobber_eflags_for_unknown_code`

!!! warning "Normally we can't follow flags through calls we can't see"

Take a virtual call, like `actor.update()` in [Function Discovery: vtables]:

```asm
; update_actor:
add  edx, 1      
mov  eax, [ecx]  ; load the vtable pointer
call [eax]       ; ...this calls the `update` method of actor interface/abstract class.
```

The problem here is, we often can't tell all the places `call [eax]` might jump
to.<br/>
May be a pointer carried through multiple functions; calculated at runtime, etc.

!!! note "Normally we'd just say 'this uses all the flags'"

    Because we have no way to really verify it...

BUT, using eflags after a function exit is the EXCEPTION, not the *RULE*.

!!! info "Even the above analysis catches only a few rare cases"

    I added that code not because it was necessary; but because other people
    might try it on different games.

With the hack on, we assume these don't use flags:

- Indirect calls and jumps, like `call [eax]`.
- Functions with unknown callers.
- Host replacements (`function_hooks`).

This only breaks hand-written asm.
Real compiler would never emit this.

## Reference Numbers

Shadow the Hedgehog, x86-64 (Clang 21.1.8).

| Build     | Code bytes | Change |
| --------- | ---------: | -----: |
| Disabled  | 18,072,503 |        |
| Enabled   | 13,011,018 | -28.0% |
| With hack | 11,993,279 | -33.6% |

[Needed Flags]: needed-flags.md
[Function Discovery: vtables]: ../../function-discovery.md#virtual-function-tables-vtables
[Needed Flags: Calls & Hooks]: needed-flags.md#calls-hooks
[FlagGraph::walk]: needed-flags.md#flaggraphwalk
