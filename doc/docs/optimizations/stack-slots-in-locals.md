# Stack Slots in Locals

Keeps the stack slots of leaf functions in a local C++ array, so Clang can
hold them in registers.

!!! danger "This is a speed hack, and it's on by default."

    Turn it off with `stack_slots_in_locals` in `[speed_hacks]`.

    The array is never copied back to guest memory.<br/>
    Shouldn't affect real game code (in any context).<br/>
    Only dirty hacks, maybe debuggers. Nothing on Xbox.

!!! note "Where this happens"

    `run` in `codegen/optimizations/stack_slots_in_locals/mod.rs`

## How it works

!!! tip "Same idea as [MMX locals](mmx-locals.md), but for the stack. So this page is a stub."

x86 only has 8 registers, so games spill to the stack a lot.<br/>
That sucks, especially for recompilation.<br/>

As each spill, e.g. `mov [esp+4], eax`, is a read or write to guest memory. (SLOOOOW!)

Clang can't always optimise it because it has to assume:

- another pointer, like `[esi+8]`, may point at the same slot
- the caller may read it after we return

So all reads/writes are a bit more expensive than they should be.

## What we do.

We give each function a local `frame` array; for everything normally on stack:

```text
 +4   argument 0        guest memory
 +0   return address    guest memory
 -4   saved ebp         frame[0xC]   <- `push ebp`
 -8   pushed ecx        frame[0x8]   <- also [ebp-4]
```

On entry, we copy `frame` in from guest memory.<br/>
On exit, we don't write it back.

**It applies when the function:**

- has no `call`, [midasm hook] or undecoded code
- has an ESP we can [track][Stack Tracking] on every path
- never leaks a stack address, like `lea ecx, [esp+4]` or `push esp`
- only touches the stack in ways we can rewrite, like `push`, `pop` or
  `mov [esp+4], eax`.

## Before and after

`sub_2B5D60` from Shadow the Hedgehog, everything else on defaults.

### Recompiled C++

=== "Before"

    ```cpp
    // push ebp
    X86_PUSH<false>(ctx, base, ctx.ebp.u32);
    // mov ebp, esp
    ctx.ebp.u32 = ctx.esp.u32;
    // push ecx
    X86_PUSH<false>(ctx, base, ctx.ecx.u32);
    // mov [ebp-0x04], ecx
    X86_NONVOLATILE_STORE_U32(static_cast<uintptr_t>(ctx.ebp.u32) - 0x4, ctx.ecx.u32);
    // mov eax, [ebp-0x04]
    ctx.eax.u32 = X86_NONVOLATILE_LOAD_U32(static_cast<uintptr_t>(ctx.ebp.u32) - 0x4);
    // sub eax, 0x0C
    X86_SUB32<0x0>(ctx.eax.u32, 12, ctx);
    // mov esp, ebp
    ctx.esp.u32 = ctx.ebp.u32;
    // pop ebp
    ctx.ebp.u32 = X86_POP<false>(ctx, base);
    ```

=== "After"

    ```cpp
    alignas(16) uint8_t frame[0x10]; // <- at the top of the function
    X86_FRAME_ENTER<0x10>(frame, ctx.esp.u32 - 0x10u, base);

    // push ebp
    X86_FRAME_PUSH(ctx, frame, 0xC, ctx.ebp.u32);
    // mov ebp, esp
    ctx.ebp.u32 = ctx.esp.u32;
    // push ecx
    X86_FRAME_PUSH(ctx, frame, 0x8, ctx.ecx.u32);
    // mov [ebp-0x04], ecx
    X86_FRAME_STORE(uint32_t, 0x8, ctx.ecx.u32);
    // mov eax, [ebp-0x04]
    ctx.eax.u32 = X86_FRAME_LOAD(uint32_t, 0x8);
    // sub eax, 0x0C
    X86_SUB32<0x0>(ctx.eax.u32, 12, ctx);
    // mov esp, ebp
    ctx.esp.u32 = ctx.ebp.u32;
    // pop ebp
    ctx.ebp.u32 = X86_FRAME_POP(ctx, frame, 0xC);
    ```

### Generated ASM

x86-64, Clang 21.1.8, `-O2 -march=sandybridge`.<br/>
`rdi` is `ctx`, `rsi` is guest memory.

=== "Before"

    ```asm
    mov  eax, dword ptr [rdi + 48]         ; ebp
    mov  ecx, dword ptr [rdi + 56]         ; esp
    lea  edx, [rcx - 4]
    mov  dword ptr [rsi + rdx], eax        ; push ebp
    mov  eax, dword ptr [rdi + 16]         ; ecx
    lea  r8d, [rcx - 8]
    mov  dword ptr [rsi + r8], eax         ; push ecx
    mov  dword ptr [rsi + rdx - 4], eax    ; mov [ebp-4], ecx
    add  eax, -12
    mov  dword ptr [rdi], eax              ; eax = ecx - 12
    add  ecx, 4
    mov  dword ptr [rdi + 56], ecx         ; ret
    ```

=== "After"

    ```asm
    mov  eax, dword ptr [rdi + 16]         ; ecx
    add  eax, -12
    mov  dword ptr [rdi], eax              ; eax = ecx - 12
    add  dword ptr [rdi + 56], 4           ; ret
    ```

[midasm hook]: ../how-it-works.md#midasm-hooks
[Stack Tracking]: helpers/stack-tracking.md
