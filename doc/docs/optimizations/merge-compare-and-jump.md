# Merged Compare and Jump

Skips writing `eflags` for branches.

!!! note "Where this happens"

    `run` in `codegen/optimizations/merge_cmp_jmp.rs`.

## Before and after

### Recompiled C++

=== "Before"

    ```cpp
    // cmp eax, 0x20
    ctx.eflags.update_compare_flags_U32(ctx.eax.u32, 0x20u);
    // jz equal
    if (X86_CC_Z(ctx)) goto equal;
    ```

=== "After"

    ```cpp
    // cmp eax, 0x20
    // jz equal
    if (ctx.eax.u32 == 0x20u) goto equal;
    ```

=== "After, one path needs flags"

    ```cpp
    // cmp cl, [edi]
    // jnz differ
    {
        const uint8_t compare_rhs = X86_LOAD_U8(ctx.edi.u32); // <- read once
        if (ctx.ecx.u8_lo != compare_rhs) {
            ctx.eflags.update_compare_flags_U8<0x1>(ctx.ecx.u8_lo, compare_rhs);
            goto differ;
        }
    }
    ```

## How it works

!!! info "On x86, a compare writes flags, then a conditional jump reads them."

```asm
cmp eax, 0x20   ; writes flags
jz  equal       ; reads ZF
```

That write of `ZF`, and other flags can be redundant in some contexts.

While Clang can sometimes optimise it out, it's not always able to do so;
especially with all of eflags shared in one variable, etc.

When we determine it's safe, we either:

- Eliminate the flag write entirely if no path needs it.
- Eliminate the flag write only on some paths.

### Step by step

!!! tip "`plan` in `merge_cmp_jmp.rs` checks each instruction in a [block]."

It's easiest to follow with an example:

```asm
    cmp cl, [edi]
    mov [esi], eax
    jnz differ
    cmp ecx, edx
    ; ...
differ:
    sbb eax, eax    ; reads CF
```

**Step 1: find a compare**

```asm
cmp cl, [edi]   ; <- compare
```

It must be `cmp`, `test`, `comiss`, `ucomiss`, `fcomip` or `fucomip`.<br/>
Operands must be 8-, 16- or 32-bit registers, immediates or `ds`/`ss` memory.

**Step 2: look at what follows**

```asm
cmp cl, [edi]
mov [esi], eax  ; no flags: goes along
jnz differ      ; reads flags: merged
cmp ecx, edx    ; writes flags: stop
```

We check up to 16 instructions after the compare.

- eflags readers (`jcc`, `setcc` and `cmovcc`) get merged.
- Instructions that don't touch the flags are left alone.

We stop looking when we reach any of:

- Another instruction that reads or writes flags, like `cmp` or `adc`.
- An eflags reader that C++ can't test directly, like `jo`.
- A call, return, `jmp` or interrupt.
- A jump target or [midasm hook].

**Step 3: check which flags each path needs**

```asm
jnz differ      ; taken:     CF    <- differ: sbb reads CF
                ; not taken: none  <- cmp ecx, edx overwrites all flags
```

[Needed Flags] gives us this.

- An integer compare merges if at least one path needs no flags.
- A float compare merges only if no path needs flags.

**Step 4: write the C++**

```cpp
const uint8_t compare_rhs = X86_LOAD_U8(ctx.edi.u32); // <- read [edi] (from `cmp cl, [edi]`)
X86_STORE_U32(ctx.esi.u32, ctx.eax.u32);              // <- from (`mov [esi], eax`)
if (ctx.ecx.u8_lo != compare_rhs) {
    //                         CF only 👇
    ctx.eflags.update_compare_flags_U8<0x1>(ctx.ecx.u8_lo, compare_rhs); 
    goto differ;
}
```

### Special case: `fstp st0` after an x87 compare

!!! note "An `fstp st0` right after `fcomip` or `fucomip` gets merged too."

```asm
fcomip st0, st1  ; compare, then pop st0
fstp   st0       ; pop again: both values are gone
jb     below     ; reads CF from fcomip
```

We do both pops right after the compare.

=== "Before"

    ```cpp
    // fcomip st0, st1
    val_double_0 = ctx.fpu.st1();
    ctx.fpu.compare_and_set_eflags<false>(ctx.fpu.st0(), val_double_0, ctx.eflags);
    ctx.fpu.pop<false>();
    // fstp st0
    ctx.fpu.pop<false>();
    // jb below
    if (X86_CC_B(ctx)) goto below;
    ```

=== "After"

    ```cpp
    // fcomip st0, st1
    // fstp st0
    {
        val_double_0 = ctx.fpu.st1();
        const bool fcomi_taken = ctx.fpu.branch_fcomi<false, 0x1>(ctx.fpu.st0(), val_double_0, ctx.eflags); // <- checks CF only
        ctx.fpu.pop<false>(); // <- from fcomip
        ctx.fpu.pop<false>(); // <- from fstp
        // jb below
        if (fcomi_taken) goto below;
    }
    ```

This sometimes pops up in code (pun intended), with x87 being stack based, we
need to keep this consistent.

[Needed Flags]: helpers/needed-flags.md
[Skip Unused Flags]: skip-unused-flags.md
[midasm hook]: ../how-it-works.md#midasm-hooks
[block]: ../function-discovery.md#local-edges
