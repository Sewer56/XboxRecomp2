# IDIV After CDQ

Uses a 32-bit divide for `idiv r/m32` when the instruction just before it is
`cdq`.

!!! note "Turn it on with `idiv32_after_cdq` in `[optimizations]`. It's on by default."

## How it works

In 32-bit x86 assembly, it's common to store 64-bit numbers as EDX:EAX.
Many instructions like `idiv` operate on this register pair.

In order to do this division with an existing signed 32-bit number (in EAX);
x86 assembly often first sign-extends it into EDX using `cdq` instruction.

i.e.

```asm
cdq             ; EDX = 0 if EAX >= 0, else 0xFFFFFFFF
idiv ebx        ; EAX = quotient (main number), EDX = remainder
```

So when you see this pair of 2 instructions, you always know you're doing
a division of a signed 32-bit number in EAX.

Now, this means we can cut some redundant calculations. So this optimisation
is all about catching this pattern and replacing `X86_IDIV32` with
cut down `X86_IDIV32_CDQ`.

**We only do this when:**

- `cdq` comes right before the `idiv`.
- No jump lands on the `idiv`, since that path may skip the `cdq`.
- The `idiv` has no [midasm hook], which could change EAX or EDX.

## Before and after

### Recompiled C++

=== "Before"

    ```cpp
    // cdq
    ctx.edx.u32 = (ctx.eax.i32 >> 31) ? 0xFFFFFFFFu : 0x00000000u;
    // idiv ebx
    X86_IDIV32(static_cast<uint32_t>(ctx.ebx.u32), ctx);
    ```

=== "After"

    ```cpp
    // cdq
    ctx.edx.u32 = (ctx.eax.i32 >> 31) ? 0xFFFFFFFFu : 0x00000000u;
    // idiv ebx
    X86_IDIV32_CDQ(static_cast<uint32_t>(ctx.ebx.u32), ctx);
    ```

### Host assembly

Clang 21.1.8, x86-64, `-O2 -march=sandybridge`.

=== "Before"

    Two divides, picked at runtime.

    ```asm
        test  rcx, rcx
        je    .skip
        shl   rdx, 32
        or    rax, rdx        ; rax = EDX:EAX
        or    rdx, rcx
        shr   rdx, 32
        je    .small
        cqo
        idiv  rcx             ; <- 64-bit divide
        jmp   .check
    .small:
        xor   edx, edx
        div   ecx
    .check:
        movsxd rcx, eax
        cmp   rcx, rax
        jne   .skip
    ```

=== "After"

    One 32-bit divide.

    ```asm
        test  ecx, ecx
        je    .skip
        lea   edx, [rax - 0x80000000]
        mov   esi, ecx
        not   esi
        or    esi, edx
        je    .skip           ; INT32_MIN / -1
        cdq
        idiv  ecx             ; <- 32-bit divide
    ```

[midasm hook]: ../how-it-works.md#midasm-hooks
