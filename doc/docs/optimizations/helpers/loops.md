# Loops

Finds the loops in one function.

!!! note "Where this happens"

    `LoopFinder::for_each_loop` in `codegen/optimizations/helpers/loops.rs`.

Used by [Nonvolatile Memory] to find wait loops.

## How we find them

```asm
    mov  esi, [ecx]
loop:
    cmp  dword ptr [esi], 0
    jz   loop
```

**Step 1: find the jumps back**

```asm
loop:               ; <- header
    ...
    jz   loop       ; <- jumps back
```

We follow every path from the start of function.<br/>
A jump to an instruction we passed on the way here closes a loop.

**Step 2: collect the loop**

```asm
loop:                        ; stop here
    cmp  dword ptr [esi], 0  ; in the loop
    jz   loop                ; start here, walk back
```

We walk back from the `jz` until we hit `loop`.<br/>
Everything we pass is in the loop.

## Giving up

!!! warning "We return `false` if:"

    - We can't follow the function's jumps.
    - Something jumps into the middle of a loop.<br/>
      We notice that when walking back reaches the function entry.

Loops found before that were already handed to the caller.<br/>
So if `false` comes back, throw away what you got.

[Nonvolatile Memory]: ../nonvolatile-memory.md
