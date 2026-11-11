# Nonvolatile Memory

By default, every guest load and store is `volatile`, because another thread
(or the GPU) might change it unexpectedly.

This speed hack drops the `volatile` where it's safe, so Clang can merge and remove them.

!!! danger "This is a speed hack, and it's on by default."

    Turn it off with `nonvolatile_memory` in `[speed_hacks]`.

    It assumes no host thread writes memory that guest code reads without a
    lock or a wait loop.

```cpp
// mov eax, [ecx+0x28]
ctx.eax.u32 = X86_LOAD_U32(ctx.ecx.u32 + 0x28);             // before
ctx.eax.u32 = X86_NONVOLATILE_LOAD_U32(ctx.ecx.u32 + 0x28); // after
```

## Exhibit A: Reading the same thing twice

!!! info "This is what we want to speed up."

```asm
mov  eax, [ecx+0x28]
mov  [ecx+0x2C], eax   ; overwritten below
mov  edx, [ecx+0x28]   ; same address again
add  eax, edx
mov  [ecx+0x2C], eax
```

=== "Before"

    ```asm
    lea  ecx, [rsi+40]
    mov  eax, dword ptr [rdi+rcx]
    add  esi, 44
    mov  dword ptr [rdi+rsi], eax   ; <- dead store
    add  eax, dword ptr [rdi+rcx]   ; <- second read
    mov  dword ptr [rdi+rsi], eax
    ```

=== "After"

    ```asm
    lea  eax, [rsi+40]
    mov  eax, dword ptr [rdi+rax]   ; read once
    add  esi, 44
    add  eax, eax
    mov  dword ptr [rdi+rsi], eax   ; stored once
    ```

## Exhibit B: Waiting on the GPU

!!! warning "Some loops wait for another thread to change memory."

```asm
; sub_3D46F0 in Shadow the Hedgehog
mov  [eax+0x100410], ecx                ; send flag to GPU
loc_3D4710:
test dword ptr [eax+0x100410], 0x10000  ; has the GPU finished?
jnz  loc_3D4710
```

Without `volatile`, Clang writes to `eax+0x100410` once; assumes value is never updated.<br/>
It never re-reads it within the loop, and the loop never ends.

## Exhibit C: Releasing a lock

!!! warning "Some locks are released with a normal `mov`."

```asm
xchg [esi+4], eax           ; take the lock
mov  [esi+8], ecx           ; do some work
mov  dword ptr [esi+4], 0   ; release it
```

Without `volatile`, Clang can move `mov  [esi+8], ecx` after `mov  dword ptr [esi+4], 0`.
So we release lock, then do stuff that was meant to be protected by the lock!

## What we do

!!! tip "We make everything non-`volatile`; except where we can't"

## What stays `volatile`

**In a loop:**

- Reads of an address that's the same every time (Exhibit B).

**In a function with such a read:**

- Every store outside the stack.<br/>
  The thread we wait on may need a store we made before the loop.

**The whole function, if:**

- It releases a lock with a normal `mov` (Exhibit C).
- We can't follow its jumps.

**Never `volatile`:**

- Code in functions without a wait loop (Exhibit A).
- Loops that read somewhere new each time:

    ```asm
    loop:
    mov  ecx, [esi]
    add  esi, 4       ; <- moves, so not a wait
    dec  edx
    jnz  loop
    ```

- Stack accesses: `[esp+N]`, or `[ebp+N]`. These are handled by `nonvolatile_stack`.

**Always `volatile`:** `xchg [mem]`, `lock xadd`, `cmpxchg` and streaming
stores like `movntps`.

### Step by step

!!! tip "`WaitingReadFinder` in `find_nonvolatile_memory/waiting_reads/` checks each function."

It's easiest to follow with an example:

```asm
    mov  esi, [ecx]
loop:
    mov  eax, [ebp-4]
    cmp  dword ptr [eax+0x10], 0
    mov  edx, [esi]
    add  esi, 4
    jz   loop
```

**Step 1: find the loops**

[Loops] finds every loop, `loop:` to `jz loop` here.

!!! warning "If [Loops] gives up, the whole function stays `volatile`."

    Like when something jumps into the middle of a loop.<br/>
    Chances are though; it may be a misanalysis from brute-force search.

**Step 2: see which registers change**

```asm
    mov  eax, [ebp-4]   ; same value every pass (never changed later)
    add  esi, 4         ; new value every pass
```

A `mov` or `lea` doesn't change a register if it copies from:

- a constant
- a register the loop doesn't change
- a stack slot the loop doesn't write

!!! success "In this example"

    `eax` copies from `[ebp-4]`.<br/>
    The loop never changes `ebp` or writes `[ebp-4]`, so only `esi` changes.

**Step 3: check each read**

```asm
    mov  eax, [ebp-4]             ; stack: skipped
    cmp  dword ptr [eax+0x10], 0  ; eax doesn't change: same address
    mov  edx, [esi]               ; esi changes: somewhere new
```

!!! success "`[eax+0x10]` is the same every pass, so that read stays `volatile`."

## Results

Loads and stores in Shadow the Hedgehog, everything else on defaults.

| Settings                                | Nonvolatile | `volatile` |
| --------------------------------------- | ----------: | ---------: |
| `nonvolatile_stack` + `find_ebp_frames` |     153,427 |    264,880 |
| + `nonvolatile_memory`                  |     383,446 |     34,861 |

[EBP frame]: find-ebp-frames.md
[Loops]: helpers/loops.md
