# Skip Unused Flags

Only computes the flags that later instructions actually read.

!!! note "Where this happens"

    `run` in `codegen/optimizations/eliminate_dead_flags.rs`.

## Summary

!!! info "On x86, math instructions write flags that often nobody reads."

    (Just like humans don't read LLM-generated garbage)

```asm
sub eax, 0      ; writes all flags
jz  one         ; reads ZF
dec eax         ; writes all flags, except CF
jz  three       ; reads ZF
dec eax         ; writes all flags, except CF
jnz one         ; reads ZF
```

Every jump reads only ZF, and nothing reads CF.<br/>
So `sub` and each `dec` only need to compute ZF.

!!! tip "Computing each flag costs host instructions"

    So we can go more brr-brr if we skip the ones nobody reads.

## Before and after

`sub_59FE0` from Shadow the Hedgehog.

### Reconstructed C++

=== "Source"

    Roughly what the original source looked like.

    ```cpp
    int sub_59FE0()
    {
        switch (field_38) // <- [ecx+0x38]
        {
            case 0:  return 1;
            case 1:  return 3;
            case 2:  return 4;
            default: return 1;
        }
    }
    ```

=== "Lowered"

    The `switch` was likely lowered to this before code generation.

    ```cpp
    int sub_59FE0()
    {
        int value = field_38;               // mov eax, [ecx+0x38]
        if (value == 0) return 1;           // sub eax, 0 + jz
        value -= 1;                         // dec eax
        if (value == 0) return 3;           // jz
        value -= 1;                         // dec eax
        if (value != 0) return 1;           // jnz
        return 4;
    }
    ```

### Recompiled C++

=== "Before"

    ```cpp
    X86_PROTECTED_FUNC(sub_59FE0) {
        // mov eax, [ecx+0x38]
        ctx.eax.u32 = X86_LOAD_U32(ctx.ecx.u32 + 0x38); // <- field_38
        // sub eax, 0x00
        X86_SUB32(ctx.eax.u32, 0, ctx);
        // jz 0x00059FFA
        if (X86_CC_Z(ctx)) goto loc_59FFA;
        // dec eax
        X86_DEC32(ctx.eax.u32, ctx);
        // jz 0x00059FF4
        if (X86_CC_Z(ctx)) goto loc_59FF4;
        // dec eax
        X86_DEC32(ctx.eax.u32, ctx);
        // jnz 0x00059FFA
        if (X86_CC_NZ(ctx)) goto loc_59FFA;
        // mov eax, 0x04
        ctx.eax.u32 = 0x4u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FF4:
        // mov eax, 0x03
        ctx.eax.u32 = 0x3u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FFA:
        // mov eax, 0x01
        ctx.eax.u32 = 0x1u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    }
    ```

=== "After"

    ```cpp
    X86_PROTECTED_FUNC(sub_59FE0) {
        // mov eax, [ecx+0x38]
        ctx.eax.u32 = X86_LOAD_U32(ctx.ecx.u32 + 0x38); // <- field_38
        // sub eax, 0x00
        X86_SUB32<0x40>(ctx.eax.u32, 0, ctx); // <- ZF only
        // jz 0x00059FFA
        if (X86_CC_Z(ctx)) goto loc_59FFA;
        // dec eax
        X86_DEC32<0x40>(ctx.eax.u32, ctx);
        // jz 0x00059FF4
        if (X86_CC_Z(ctx)) goto loc_59FF4;
        // dec eax
        X86_DEC32<0x40>(ctx.eax.u32, ctx);
        // jnz 0x00059FFA
        if (X86_CC_NZ(ctx)) goto loc_59FFA;
        // mov eax, 0x04
        ctx.eax.u32 = 0x4u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FF4:
        // mov eax, 0x03
        ctx.eax.u32 = 0x3u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FFA:
        // mov eax, 0x01
        ctx.eax.u32 = 0x1u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    }
    ```

    Its callers don't read flags after `ret`, so ZF is all we need.

=== "Inlined"

    As inlined into `sub_5AAF0` by Clang.

    ```cpp
    X86_WEAK_FUNC(sub_5AAF0) {
        // ...
    loc_5AD91:
        // mov ecx, esi
        ctx.ecx.u32 = ctx.esi.u32;
        // call 0x00059FE0
        ctx.SubEsp(1); //Adjust stack pointer for call instruction
        sub_59FE0(ctx, base); // <- inlined here
        // pop edi
        ctx.edi.u32 = X86_POP<false>(ctx, base);
        // ...
    }
    ```

=== "Hand-written"

    Written by hand for comparison.

    ```cpp
    uint32_t sub_59FE0(uint32_t ecx, uint8_t* base)
    {
        uint32_t eax = X86_LOAD_U32(ecx + 0x38); // <- field_38
        if (eax == 0) return 1;
        eax -= 1;
        if (eax == 0) return 3;
        eax -= 1;
        if (eax != 0) return 1;
        return 4;
    }
    ```

=== "Param + local ctx"

    The After body, with `ecx` passed in and `eax` returned.

    ```cpp
    uint32_t sub_59FE0(uint32_t ecx, uint8_t* base)
    {
        X86Context ctx{}; // <- local
        ctx.ecx.u32 = ecx;
        // mov eax, [ecx+0x38]
        ctx.eax.u32 = X86_LOAD_U32(ctx.ecx.u32 + 0x38); // <- field_38
        // sub eax, 0x00
        X86_SUB32<0x40>(ctx.eax.u32, 0, ctx);
        // jz 0x00059FFA
        if (X86_CC_Z(ctx)) goto loc_59FFA;
        // dec eax
        X86_DEC32<0x40>(ctx.eax.u32, ctx);
        // jz 0x00059FF4
        if (X86_CC_Z(ctx)) goto loc_59FF4;
        // dec eax
        X86_DEC32<0x40>(ctx.eax.u32, ctx);
        // jnz 0x00059FFA
        if (X86_CC_NZ(ctx)) goto loc_59FFA;
        // mov eax, 0x04
        ctx.eax.u32 = 0x4u;
        return ctx.eax.u32;
    loc_59FF4:
        // mov eax, 0x03
        ctx.eax.u32 = 0x3u;
        return ctx.eax.u32;
    loc_59FFA:
        // mov eax, 0x01
        ctx.eax.u32 = 0x1u;
        return ctx.eax.u32;
    }
    ```

=== "Merged branches"

    The After body by hand, with branches reading `eax` instead of flags.

    ```cpp
    X86_PROTECTED_FUNC(sub_59FE0) {
        // mov eax, [ecx+0x38]
        ctx.eax.u32 = X86_LOAD_U32(ctx.ecx.u32 + 0x38); // <- field_38
        // sub eax, 0x00
        // jz 0x00059FFA
        if (ctx.eax.u32 == 0) goto loc_59FFA; // <- no flags
        // dec eax
        ctx.eax.u32 -= 1;
        // jz 0x00059FF4
        if (ctx.eax.u32 == 0) goto loc_59FF4;
        // dec eax
        ctx.eax.u32 -= 1;
        // jnz 0x00059FFA
        if (ctx.eax.u32 != 0) goto loc_59FFA;
        // mov eax, 0x04
        ctx.eax.u32 = 0x4u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FF4:
        // mov eax, 0x03
        ctx.eax.u32 = 0x3u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    loc_59FFA:
        // mov eax, 0x01
        ctx.eax.u32 = 0x1u;
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    }
    ```

### Host assembly

Clang 21.1.8, x86-64, `-O2 -march=sandybridge`.

| Version           | Instructions |
| ----------------- | ------------ |
| Before            | 82           |
| After             | 33           |
| After (Inlined)   | 28           |
| Original          | 13           |
| Hand-written      | 13           |
| Param + local ctx | 13           |
| Merged branches   | 16           |

Note that after inlining, the 3 instruction difference for `Merged branches`
disappears; as Clang can optimise away the stack adjustment.

=== "Before"

    ```asm
    push   rbp
    mov    rbp, rsp
    mov    eax, DWORD PTR [rdi+16]       ; guest ecx
    add    eax, 56                       ; + 0x38
    mov    eax, DWORD PTR [rsi+rax]      ; load field_38

    ; sub eax, 0
    xor    ecx, ecx
    test   eax, eax
    sete   cl                            ; ZF
    shl    ecx, 6
    mov    edx, eax
    shr    edx, 24
    and    edx, -128                     ; SF
    xor    esi, esi
    test   al, al
    setnp  sil                           ; PF
    or     edx, ecx
    lea    ecx, [rdx+4*rsi]
    xor    ecx, 4
    mov    DWORD PTR [rdi+84], ecx       ; store guest EFLAGS
    test   eax, eax
    je     .LBB0_5                       ; jz

    ; dec eax
    xor    edx, edx
    mov    ecx, eax
    dec    ecx
    sete   dl                            ; ZF
    shl    edx, 6
    mov    esi, ecx
    shr    esi, 24
    and    esi, -128                     ; SF
    xor    r8d, r8d
    test   cl, cl
    setnp  r8b                           ; PF
    xor    r9d, r9d
    mov    r10d, eax
    neg    r10d
    seto   r9b                           ; OF
    shl    r9d, 11
    mov    r10d, ecx
    xor    r10d, eax
    and    r10d, 16                      ; AF
    or     r10d, r9d
    or     r10d, esi
    or     r10d, edx
    lea    edx, [r10+4*r8]
    xor    edx, 4
    mov    DWORD PTR [rdi+84], edx       ; store guest EFLAGS
    test   ecx, ecx
    je     .LBB0_2                       ; jz

    ; dec eax
    xor    edx, edx
    add    eax, -2
    sete   dl                            ; ZF
    shl    edx, 6
    mov    esi, eax
    shr    esi, 24
    and    esi, -128                     ; SF
    xor    r8d, r8d
    test   al, al
    setnp  r8b                           ; PF
    xor    r9d, r9d
    mov    r10d, ecx
    neg    r10d
    seto   r9b                           ; OF
    shl    r9d, 11
    xor    ecx, eax
    and    ecx, 16                       ; AF
    or     ecx, r9d
    or     ecx, esi
    or     ecx, edx
    lea    ecx, [rcx+4*r8]
    xor    ecx, 4
    mov    DWORD PTR [rdi+84], ecx       ; store guest EFLAGS
    test   eax, eax
    je     .LBB0_4                       ; jnz (Clang flipped it)

    .LBB0_5:
    mov    eax, 1
    jmp    .LBB0_6
    .LBB0_2:
    mov    eax, 3
    jmp    .LBB0_6
    .LBB0_4:
    mov    eax, 4
    .LBB0_6:
    mov    DWORD PTR [rdi], eax          ; guest eax
    add    DWORD PTR [rdi+56], 4         ; guest esp += 4
    pop    rbp
    ret
    ```

=== "After"

    ```asm
    push   rbp
    mov    rbp, rsp
    mov    eax, DWORD PTR [rdi+16]       ; guest ecx
    add    eax, 56                       ; + 0x38
    mov    eax, DWORD PTR [rsi+rax]      ; load field_38

    ; sub eax, 0
    xor    ecx, ecx
    test   eax, eax
    sete   cl                            ; ZF
    shl    ecx, 6
    mov    DWORD PTR [rdi+84], ecx       ; store guest EFLAGS
    test   eax, eax
    je     .LBB0_3                       ; jz

    ; dec eax
    xor    ecx, ecx
    cmp    eax, 1
    sete   cl                            ; ZF
    shl    ecx, 6
    mov    DWORD PTR [rdi+84], ecx       ; store guest EFLAGS
    mov    ecx, 3
    cmp    eax, 1
    je     .LBB0_4                       ; jz

    ; dec eax
    xor    ecx, ecx
    cmp    eax, 2
    sete   cl                            ; ZF
    shl    ecx, 6
    mov    DWORD PTR [rdi+84], ecx       ; store guest EFLAGS
    mov    ecx, 4
    cmp    eax, 2
    je     .LBB0_4                       ; jnz (Clang flipped it)

    .LBB0_3:
    mov    ecx, 1
    .LBB0_4:
    mov    DWORD PTR [rdi], ecx          ; guest eax
    add    DWORD PTR [rdi+56], 4         ; guest esp += 4
    pop    rbp
    ret
    ```

=== "Inlined"

    As inlined into `sub_5AAF0` by Clang.

    ```asm
    mov    ecx, DWORD PTR [rbx+32]       ; guest esi (from `mov ecx, esi`)
    add    ecx, 56                       ; + 0x38
    mov    ecx, DWORD PTR [r14+rcx]      ; load field_38

    ; sub eax, 0
    xor    edx, edx
    test   ecx, ecx
    sete   dl                            ; ZF
    shl    edx, 6
    mov    DWORD PTR [rbx+84], edx       ; store guest EFLAGS
    test   ecx, ecx
    je     .LBB78_56                     ; jz

    ; dec eax
    xor    edx, edx
    cmp    ecx, 1
    sete   dl                            ; ZF
    shl    edx, 6
    mov    DWORD PTR [rbx+84], edx       ; store guest EFLAGS
    mov    edx, 3
    cmp    ecx, 1
    je     .LBB78_57                     ; jz

    ; dec eax
    xor    edx, edx
    cmp    ecx, 2
    sete   dl                            ; ZF
    shl    edx, 6
    mov    DWORD PTR [rbx+84], edx       ; store guest EFLAGS
    mov    edx, 4
    cmp    ecx, 2
    je     .LBB78_57                     ; jnz (Clang flipped it)

    .LBB78_56:
    mov    edx, 1
    .LBB78_57:
    mov    DWORD PTR [rbx], edx          ; guest eax
    ```

=== "Original"

    From the original `.xbe`.

    ```asm
    mov    eax, DWORD PTR [ecx+0x38]     ; load field_38
    sub    eax, 0
    jz     loc_59FFA
    dec    eax
    jz     loc_59FF4
    dec    eax
    jnz    loc_59FFA
    mov    eax, 4
    ret

    loc_59FF4:
    mov    eax, 3
    ret

    loc_59FFA:
    mov    eax, 1
    ret
    ```

=== "Hand-written"

    ```asm
    push   rbp
    mov    rbp, rsp
    add    edi, 56                       ; ecx + 0x38
    mov    eax, DWORD PTR [rsi+rdi]      ; load field_38
    xor    ecx, ecx
    cmp    eax, 1
    sete   cl
    lea    ecx, [2*rcx+1]                ; 1 -> 3, else 1
    cmp    eax, 2
    mov    eax, 4
    cmovne eax, ecx                      ; 2 -> 4
    pop    rbp
    ret
    ```

=== "Param + local ctx"

    ```asm
    push   rbp
    mov    rbp, rsp
    add    edi, 56                       ; ecx + 0x38
    mov    eax, DWORD PTR [rsi+rdi]      ; load field_38
    xor    ecx, ecx
    cmp    eax, 1
    sete   cl
    lea    ecx, [2*rcx+1]                ; 1 -> 3, else 1
    cmp    eax, 2
    mov    eax, 4
    cmovne eax, ecx                      ; 2 -> 4
    pop    rbp
    ret
    ```

=== "Merged branches"

    ```asm
    push   rbp
    mov    rbp, rsp
    mov    eax, DWORD PTR [rdi+16]       ; guest ecx
    add    eax, 56                       ; + 0x38
    mov    eax, DWORD PTR [rsi+rax]      ; load field_38
    xor    ecx, ecx
    cmp    eax, 1
    sete   cl
    lea    ecx, [2*rcx+1]                ; 1 -> 3, else 1
    cmp    eax, 2
    mov    eax, 4
    cmovne eax, ecx                      ; 2 -> 4
    mov    DWORD PTR [rdi], eax          ; guest eax
    add    DWORD PTR [rdi+56], 4         ; guest esp += 4
    pop    rbp
    ret
    ```

## How it works

[Needed Flags] is used to work out which flags are still needed after each
instruction.

When we write the C++, these flags are passed as a mask:

```cpp
X86_SUB32<0x40>(ctx.eax.u32, 0, ctx); // <- 0x40 is ZF
```

e.g. 

- `<0x0>` == no flags.
- No mask -> all six (CF, PF, AF, ZF, SF and OF).

!!! note "Calls and `ret` can (in theory) pass flags to other functions."

    Emphasis on 'in theory', as in practice `eflags` is assumed to be clobbered.
    Only handrolled ASM assumes eflags is passed.

[Merged Compare and Jump]: merge-compare-and-jump.md
[Flag Boundaries]: helpers/flag-boundaries.md
[Flag Effects]: helpers/flag-effects.md
[Flag Effects: Narrowing]: helpers/flag-effects.md#narrowing
[Needed Flags]: helpers/needed-flags.md
