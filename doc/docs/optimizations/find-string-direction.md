# Find String Direction

Works out which way string instructions like `rep stosd` run, so the C++
doesn't have to check at runtime.

!!! note "Intel calls these 'String Instructions', but they're really memory copy/compare/fill intrinsics."

## How it works

!!! info "String instructions (`movs`, `stos`, `cmps`, `scas`) walk memory forward or backward, depending on the direction flag (DF)."

```asm
cld             ; DF = 0
rep stosd       ; fills [edi], [edi+4], [edi+8], ...
std             ; DF = 1
rep stosd       ; fills [edi], [edi-4], [edi-8], ...
```

Compilers like MSVC keep DF at 0 between functions.

!!! tip "So we assume DF = 0 when a function starts and after each `call`, then track `std` (sets DF = 1) and `cld` (sets DF = 0)."

!!! warning "If a function could call or return with DF = 1, we turn this off for the whole game."

    It's a safety net, in case a game's compiler doesn't follow that rule.<br/>
    The log then says `WARNING: DF may be set after ...`.

## Before and after

`sub_56D80` in Shadow the Hedgehog sets 108 bytes to `0xFF`:

```asm
or   eax, 0xFFFFFFFF  ; value to write
mov  ecx, 0x1B        ; 27 writes of 4 bytes
mov  edi, 0x541EA8    ; where to start
rep  stosd
```

### Recompiled C++

=== "Before"

    ```cpp
    if (ctx.eflags.rare & X86EFlags::DF) { // <- checked at runtime
        for (uint32_t i = 0; i < ctx.ecx.u32; ++i) {
            X86_STORE_U32(static_cast<uintptr_t>(ctx.edi.u32) - i * 4, ctx.eax.u32);
        }
        ctx.edi.u32 -= ctx.ecx.u32 * 4;
    } else {
        // the same loop, going forward
    }
    ```

=== "After"

    ```cpp
    if (false) { // <- Clang deletes this branch
        for (uint32_t i = 0; i < ctx.ecx.u32; ++i) {
            X86_STORE_U32(static_cast<uintptr_t>(ctx.edi.u32) - i * 4, ctx.eax.u32);
        }
        ctx.edi.u32 -= ctx.ecx.u32 * 4;
    } else {
        // the same loop, going forward
    }
    ```

### Host assembly

Clang 21.1.8, x86-64, `-O2 -march=sandybridge`.

=== "Before"

    ```asm
    ; Clang unrolls both loops, then picks each address with a cmov.
    test  r9d, 0x400
    lea   rax, [rsi+0x541f0c]
    lea   rcx, [rsi+0x541e44]
    cmove rcx, rax
    ; ... 22 more cmovs, then the stores
    ```

=== "After"

    ```asm
    ; Just the stores, 4 bytes each.
    mov   dword ptr [rsi+0x541ea8], 0xffffffff
    mov   dword ptr [rsi+0x541eac], 0xffffffff
    mov   dword ptr [rsi+0x541eb0], 0xffffffff
    ; ...
    ```

The whole function shrinks from 252 to 113 instructions.

## memmove and memset

!!! info "The `rep_memmove_memset` speed hack runs `rep movs` and `rep stos` as `memmove` and `memset`."

Those copy 16 or 32 bytes at a time, while our loop copies 1, 2 or 4.

```cpp
// rep movsd (`false` means forward)
if (!x86_rep_movs_memmove<uint32_t>(base, ctx.esi.u32, ctx.edi.u32, ctx.ecx.u32, false))
{
    // the usual loop
}
```

**The usual loop still runs when:**

- The copy is under 16 bytes, and the size isn't a constant. (slower)
- `rep movs` overlaps, with the destination ahead. (differs from `memmove`)
- `rep stos` fills with a value that isn't one repeated byte, like `0x00410041`.<br/>
  `memset` can only repeat one byte.

!!! warning "This needs the `native_sized_addresses` speed hack on too."

    Without it, guest addresses wrap at 4 GB, and `memmove` can't do that.
