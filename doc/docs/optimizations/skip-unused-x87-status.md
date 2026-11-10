# Skip Unused x87 Status

Skips the x87 status-bit (C0-C3) writes that nothing reads.

!!! note "Where this happens"

    `run` in `codegen/optimizations/eliminate_dead_x87_status.rs`.

!!! warning "This is 99.99999% superseeded."

    [By lifting `x87` to locals](x87.md); Clang already eliminates
    these.<br/> So this only handles `<2-3%` of code we can't optimise.

    But throwing away the prototype code would have been a waste, so here's
    this little 'stub' of a page.

## How it works

!!! info "We'll use 'C1' as an example, but also applies to C0-C3."

Most x87 instructions set the C1 status bit to 0, written as
`fpu.sw &= ~X87FPU::C1;`.

A few instructions like `fnstsw`, `fnstenv`, `fnsave` and `fxsave` actually read it.<br/>
Some compare instructions like `fxam` set it to their own value.

```asm
fld  [ecx]   ; clears C1
fmul [edx]   ; clears C1
fadd [esi]   ; clears C1
ret
```

We walk each function backward, like [Skip Unused Flags] does for eflags.

## Before and after

`sub_30ECB0` from Shadow the Hedgehog (vector length squared):

=== "Before"

    ```cpp
    // fld [ecx+0x08]
    fpu.sw &= ~X87FPU::C1;
    fpu.push<false>(X86_LOAD_F32(ctx.ecx.u32 + 0x8));
    // fld [ecx+0x04]
    fpu.sw &= ~X87FPU::C1;
    fpu.push<false>(X86_LOAD_F32(ctx.ecx.u32 + 0x4));
    ```

=== "After"

    ```cpp
    // fld [ecx+0x08]
    fpu.push<false>(X86_LOAD_F32(ctx.ecx.u32 + 0x8));
    // fld [ecx+0x04]
    fpu.push<false>(X86_LOAD_F32(ctx.ecx.u32 + 0x4));
    ```

[Skip Unused Flags]: skip-unused-flags.md
