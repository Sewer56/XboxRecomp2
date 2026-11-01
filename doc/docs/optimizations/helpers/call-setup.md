# Call Setup

Finds arguments written to the stack before a call.

!!! note "Where this happens"

    `CallSetup::find` in `codegen/optimizations/helpers/call_setup.rs`.

## Rationale

!!! info "Callers write each argument to the stack right before the call"

Usually via `push` instructions.

But sometimes we can be a bit more clever; and pass them via registers instead.
See [Stack Argument Elimination].

This helper finds the instructions that write each argument to the stack
before a call.

## What we return

!!! info "A list of instructions that write to stack. Usually `push`."

```rust
struct CallSetup {
    start: BlockOpIndex,    // Earliest write.
    captures: Vec<Capture>,
}

struct Capture {
    index: BlockOpIndex,    // The write.
    argument: u8,           // Argument number. 0 is `[esp]` at the call.
    omit_write: bool,       // Nothing reads the write, so we can remove it.
}
```

For example:

```asm
; index  code                  (the block containing the call)
; 0      sub  esp, 8
; 1      mov  [esp], eax      ; writes argument 0
; 2      mov  [esp+4], edx    ; writes argument 1
; 3      mov  ecx, [esp+4]    ; reads argument 1 back
;        call f
```

| `index` | `argument` | `omit_write` |
| ------- | ---------- | ------------ |
| 2       | 1          | `false`      |
| 1       | 0          | `true`       |

Here, `start` is 1.

## How we find them

We walk back from the call, one instruction at a time.<br/>
Along the way, we track where ESP was relative to the call.

### Capture

The first whole-argument write we meet per argument, closest to the call,
becomes a `Capture`:

- `push imm`, `push reg32` or `push dword [mem]`.
- `mov [esp+N], reg32/imm`.
- `movss [esp+N], xmm`.

### Keep going past

- Other stack writes, like `mov byte [esp], 1`.
    - The argument stays on the stack, with no `Capture`.
- Stack adjustments: `add/sub esp, N`.
- Other simple instructions, like `mov`, `lea` or `add`, that:
    - Don't write memory.
    - Don't write ESP.

### Stop at

- The start of the block.
- The point where every argument we want has a write.
- An unknown memory write, like `mov [edx], eax`.
- Any other write to ESP, like `mov sp, ax`.
- Any other instruction, like a call.

[Stack Argument Elimination]: ../stack-argument-elimination.md
