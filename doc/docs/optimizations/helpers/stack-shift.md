# Stack Shift

Moves every stack access above some point by a fixed number of bytes.

!!! note "Where this happens"

    `StackShift::find` and `StackShift::apply` in
    `codegen/optimizations/helpers/stack_shift.rs`.

## Rationale

!!! info "Some optimizations add or remove stack space"

This means we need to adjust all instructions.

Say we remove the return address, like [Return Slot Elimination] does.<br/>
So everything from entry ESP + 4 (argument 0) moves down by 4.
That +4 is the boundary.

One function, top to bottom:

=== "Before"

    ```asm
    sub  esp, 4             ; make room for a local
    mov  ecx, [esp]         ; the local
    mov  eax, [esp+8]       ; argument 0
    lea  edx, [esp+12]      ; points at argument 1
    mov  eax, [edx]         ; reads argument 1
    mov  edx, 0             ; drops the pointer
    add  esp, 4
    ret  8
    ```

=== "After"

    ```asm
    sub  esp, 4
    mov  ecx, [esp]         ; below the boundary: stays put
    mov  eax, [esp+4]       ; moved down 4
    lea  edx, [esp+8]       ; moved down 4
    mov  eax, [edx]         ; follows the pointer: no change needed
    mov  edx, 0
    add  esp, 4
    ret  8                  ; count unchanged, see below
    ```

## How we shift

!!! info "We use [Stack Tracking] to first get ESP and EBP before each instruction."

Afterwards, we use that information to shift every instruction.

| Entry ESP offset | Holds          | Shifted? |
| ---------------- | -------------- | -------- |
| -4               | a local        | no       |
| +0               | return address | no       |
| +4               | argument 0     | yes      |
| +8               | argument 1     | yes      |

The original `esp+0` acts as the boundary.
Anything above shifted, below not shifted.

Works with (almost) any instruction, e.g.

- `add [esp+4], eax` or
- `fld qword [esp+8]`

!!! warning "Some instructions aren't supported"

    A few can reach memory past the address they name.<br/>
    Like `bt [esp], ecx`, which reads `[esp+4]` when ECX is 32.

### Moving `lea`

`lea` can be moved too, but we need to be careful with it.

That address could go anywhere, like into another function.
So we follow the register that receives its result, and
give up if we lose track of it.

```asm
lea  eax, [esp+4]    ; moves to [esp+8]
mov  ecx, [eax+4]    ; reads argument 1, still correct
mov  eax, 0          ; pointer gone: safe
```

Particulatly, we look for this 👆.
Where the register is overwritten entirely.

------------

An extra rule applies. As per above, all uses must stay on same side of
the `[esp+0]` boundary, per rule above.

```asm
lea  eax, [esp+0]    ; unsafe!
mov  ecx, [eax+4]    ; unsafe because above is unsafe!
```

Copies, like `mov edx, eax` or `lea edx, [eax+4]`, and changes, like
`add eax, 4`, aren't supported.

### Giving up

!!! warning "We change nothing if we see:"

    - ESP, or EBP while it points into the stack, used:
        - As a value, like `mov eax, esp`.
        - As an index, like `[esp+ecx*4]`.
    - An access we can't shift safely:
        - One crossing the boundary, like a 4-byte read at `+2`.
        - An unsupported instruction, like `bt [esp], ecx`.
        - A `lea` pointer we lose track of.
    - `push`, `pop`, `leave` or `ret` touching the shifted area.
    - A `call`.
    - When shifting down, anything but `ret` using the space we move into.

## Applying the shift

Call `find`, then `apply` with the same model.

`ret N` is unchanged.
So an optimization changing the stack size will need to do it manually.

[Stack Tracking]: stack-tracking.md
[Return Slot Elimination]: ../return-slot-elimination.md
