# Entry Loads

Finds arguments read off the stack when a function starts.

!!! note "Where this happens"

    `EntryLoad::find_all` in `codegen/optimizations/helpers/entry_loads.rs`.

## Rationale

!!! info "Functions often copy stack arguments into registers right away"

Sometimes we can optimise that away.
Like with [Stack Argument Elimination].

To do that, we need to know which arguments get loaded, and into which
registers.

## What we return

One `EntryLoad` per load, in the order the loads run.
An empty `Vec` means there are none.

```rust
struct EntryLoad {
    argument: u8,            // Which stack argument it reads. 0 is the first.
    register: Register,      // Where the argument arrives.
    address: GuestAddress32, // The load instruction.
}
```

For example:

```asm
; address  code
; 0x1000   push esi
; 0x1001   mov  ecx, [esp+12]   ; argument 1 arrives in ecx
; 0x1005   mov  eax, [esp+8]    ; argument 0 arrives in eax
; 0x1009   ret
```

| `argument` | `register` | `address` |
| ---------- | ---------- | --------- |
| 1          | `ecx`      | `0x1001`  |
| 0          | `eax`      | `0x1005`  |

## How we find them

We walk forward from the entry, one instruction at a time.

### Keep going past

- Stack setup: `push reg`, `add/sub esp, N` and `mov ebp, esp`.
- Whole-argument loads: `mov reg32, [esp+N]` or `movss xmm, [esp+N]`.
  Each one becomes an `EntryLoad`.
- Other simple instructions, like `add` or `test`, that don't write memory,
  ESP or EBP.

```asm
mov  eax, [esp+4]    ; entry load: argument 0 arrives in eax
add  eax, 0x14       ; simple instruction: keep going
ret                  ; stop
```

### Stop at

Anything else:

- Calls, branches and `ret`.
- Places where two paths meet.
- A load into ESP or EBP.
- A second load of an argument we already have.
- Something that pops into the caller's stack, like `add esp, 4` right at
  the start.
- Any other instruction, like a write to memory.

### Stop if the register was already used

The register must still hold the caller's value when we load into it.
So if an earlier instruction touched it, we stop.

```asm
mov  eax, [esp+4]    ; fine: argument 0 arrives in eax
push edx             ; saves the caller's edx
mov  edx, [esp+12]   ; stop: argument 1 can't arrive in edx
```

Otherwise `push edx` would save argument 1, not the caller's `edx`.

[Stack Argument Elimination]: ../stack-argument-elimination.md
