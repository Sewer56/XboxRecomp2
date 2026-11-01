# Flag Effects

Finds which EFLAGS one instruction reads and writes.

!!! note "Where this happens"

    `EFlagEffects::of` and `EFlagEffects::can_narrow`.

## Rationale

!!! info "Every eflags-related optimization needs to know what an instruction does to eflags"

For example, `cmp` replaces every flag.
Meaning that if nothing before needs the flags, we can skip the flag writes.

## Reads and writes

!!! info "`EFlagEffects::of` gives the flags one guest instruction reads and writes."

Maybe best show by example:

| Instruction    | Reads | Always writes | Why                                     |
| -------------- | ----- | ------------- | --------------------------------------- |
| `add eax, ebx` | none  | all           |                                         |
| `adc ecx, 0`   | CF    | all           | Adds the carry flag.                    |
| `je skip`      | ZF    | none          |                                         |
| `inc eax`      | none  | all but CF    | `inc` keeps CF.                         |
| `stc`          | none  | CF            |                                         |
| `shl eax, 4`   | none  | all           | `count != 0` replaces all flags.        |
| `shl eax, cl`  | none  | none          | CL may be 0, and then nothing changes.  |
| `shl eax, 32`  | none  | none          | x86 uses `count & 31`, giving 0.        |
| `rcr eax, 1`   | CF    | CF, OF        | `rcr` sets only CF & OF                 |
| `div ecx`      | none  | none          | eflags not touched                      |
| `rcl eax, 1`   | all   | none          | We don't support- default to 'read all' |

"All" means the six arithmetic eflags: CF, PF, AF, ZF, SF and OF.

## Narrowing

!!! info "`EFlagEffects::can_narrow` says which instructions can compute fewer flags."

    By 'instructions', I mean the helpers in `x86_context_core.h`. 

For example `X86_ADD32<0x40>()`, where the `0x40` refers to the only flags we
need to compute.

But this may be a bit hard to understand... so an example may help:

```asm
add  eax, ebx    ; mask: ZF   <- only setz reads add's flags
setz cl          ; reads ZF
cmp  ecx, edx    ; replaces every flag
```

```cpp
X86_ADD32<0x40>(ctx.eax.u32, ctx.ebx.u32, ctx); // <- 0x40 is ZF
```

In this case, `cmp` replaces every flag.
Normally `add` writes all the arithmetic flags: CF, PF, AF, ZF, SF and OF.

But, the only instruction between `add` and `cmp` is `setz`, which only reads ZF.
So we can skip everything, except for ZF, when computing flags after `add`.
