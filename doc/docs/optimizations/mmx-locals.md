# MMX Locals

Move MMX registers to local array, so clang can optimise a bit better, i.e.:

- Keep them in host's registers
- Assume a clean slate/set of values (rather than having to preserve state across funcs)

!!! note "Where this happens"

    `run` in `codegen/optimizations/mmx_locals.rs`.

## How it works

!!! tip "Same idea as [x87 locals](x87.md). So this page is kinda a stub, mkay?"

MMX registers (`mm0` to `mm7`) normally live in `ctx.fpu.mmx`.<br/>
Other functions might read them, so clang must keep every write.

When it's safe, we move them to a local `uint64_t mmx[8]`.<br/>
Then clang can keep them in CPU registers and drop writes nothing reads.

It's safe when:

- `mm` register isn't a param
- called functions don't need it.
- no mid-assembly hooks (<= kinda overkill, can do syncs, but alas, not needed rn)

!!! warning "Assumption"

    We assume callers don't read the `mm` values a function leaves.<br/>
    This may be specific to MSVC. I've not verified it across a variety
    of games, so tread caution.

    Consider this 'experimental', nya~ 🐱

## Before and after

`sub_4023C0` from Shadow the Hedgehog.

=== "Before"

    ```cpp
    // movq mm0, [eax]
    ctx.fpu.mmx[0] = X86_LOAD_U64(ctx.eax.u32);
    // pavgb mm0, [ebx]
    uint64_t dst_val_4203511 = MMX_REG(0);
    uint64_t src_val_4203511 = X86_MMX_LOAD(ctx.ebx.u32);
    MMX_REG(0) = X86_PAVGB(dst_val_4203511, src_val_4203511);
    // movq [ecx], mm0
    X86_STORE_U64(ctx.ecx.u32, ctx.fpu.mmx[0]);
    ```

=== "After"

    ```cpp
    uint64_t mmx[8]; // <- at the top of the function

    // movq mm0, [eax]
    mmx[0] = X86_LOAD_U64(ctx.eax.u32);
    // pavgb mm0, [ebx]
    uint64_t dst_val_4203511 = mmx[0];
    uint64_t src_val_4203511 = X86_MMX_LOAD(ctx.ebx.u32);
    mmx[0] = X86_PAVGB(dst_val_4203511, src_val_4203511);
    // movq [ecx], mm0
    X86_STORE_U64(ctx.ecx.u32, mmx[0]);
    ```

[mid-assembly hook]: ../how-it-works.md#midasm-hooks
