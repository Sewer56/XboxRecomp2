# Stack Arguments

Works out how many stack arguments a function takes, and who removes them.

!!! note "Where this happens"

    `StackArguments::find` in `codegen/optimizations/helpers/stack_arguments.rs`.

## Where arguments sit

!!! info "The typical stack during a function call looks like this:"

    ```cpp
    int add(int a, int b) { return a + b; }

    add(1, 2);
    ```

| When `add` starts | Holds           |
| ----------------- | --------------- |
| `[esp]`           | return address  |
| `[esp+4]`         | `a`: argument 0 |
| `[esp+8]`         | `b`: argument 1 |

The caller pushes the arguments, then `call` pushes the return address.

!!! note "One of our optimisations affects this"

    If [Return Slot Elimination] might remove return address, leaving `a` at `[esp]`.

## Finding them

!!! info "In assembly, many function openings look something like this:"

    Each one reads argument 0.

Straight to the load.

```asm
mov  eax, [esp+4]
```

After a `push`.

```asm
push esi
mov  eax, [esp+8]
```

After making room for locals.

```asm
sub  esp, 0x10
mov  eax, [esp+0x14]
```

Through a frame pointer.

```asm
push ebp
mov  ebp, esp
mov  eax, [ebp+8]
```

The stack, of course moves with some instructions.
But [Stack Tracking] helps us keep track of that.

## Step by step

### 1. Read the `ret`

The `ret` tells us who removes the arguments:

- `ret 8`: the callee does. That's 2 arguments.
    - Typical for `__stdcall` and `__thiscall`.
- `ret` or `ret 0`: the caller does.
    - Typical for `__cdecl`.

Every reachable `ret` must agree, or we can't be (100%) certain.

!!! note "Well, there are someeeeeeee ways"

    But this is such a rare exception, it's not worth engineering code for
    them; nevermind the fact I've been crunching this, y'know!

### 2. Count arguments when the caller removes them

!!! info "When the caller removes the arguments"

A plain `ret` doesn't say how many arguments there are. (`cdecl` case)<br/>

In such cases, we look at stack accesses and count the highest argument
the function loads.

I use a simple heuristic: only a `mov` or `movss` that reads one whole argument
counts.

For example, `mov eax, [esp+8]` at entry loads argument 1, so the count is 2.

!!! warning "Sometimes functions can take unused arguments."

    We can't catch these.

!!! note "We handle up to 8 arguments."

    Just how many registers we have, no point tracking more for our
    optimisation purposes.

[Stack Tracking]: stack-tracking.md
[Return Slot Elimination]: ../return-slot-elimination.md
