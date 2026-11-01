# Known Callers

Prevents breaking the ABI of functions we can't safely rewrite.

!!! note "Where this happens"

    `known_callers::retain` in `codegen/optimizations/helpers/known_callers.rs`.

## Rationale

!!! danger "Change all callers, or none"

Some passes change a function's ABI (parameters, return address, etc.).<br/>
Like [Stack Argument Elimination] or [Return Slot Elimination].

Every caller has to change with it.<br/>
But we can't rewrite callers we never saw.

Example:

```asm
; A passes f as a callback.
push 0x20000        ; maybe a constant, maybe f's address
call run_callback

; run_callback:
mov  eax, [esp+4]   ; read the callback
push 5              ; argument 0 for f
call eax            ; calls f
```

But the same push could just be a number:

```asm
; B allocates 128 KiB.
push 0x20000        ; size in bytes
call malloc
```

Same applies to other brute force strategies.

We can only safely rewrite a function if it was (only) found through
direct `call` instructions we can see.

## When we drop a function

- Nothing calls it.
- Something else refers to it, like `push 0x20000` or a tail-call `jmp`.
- Discovery found it some other way, like a symbol or most [brute-force scans].
- The host controls it, or one of its callers:
    - Listed in [`function_hooks`][Whole-function replacements].
    - Has a [`[[midasm_hook]]`][Midasm Hooks].
- A call to it is the last instruction in its function.
    - Discovery may have cut that function short.
- The pass's own check fails for a call.

!!! tip "This is why accurate [function discovery] is kinda important!"

## After the change

The pass calls `FunctionModel::protect_abi` on each function it changes.

That moves its body to `<name>_fastcall`, which our rewritten calls use.
`<name>` keeps the old convention for every other caller.

See [Whole-function replacements].

[Stack Argument Elimination]: ../stack-argument-elimination.md
[Return Slot Elimination]: ../return-slot-elimination.md
[Whole-function replacements]: ../../how-it-works.md#whole-function-replacements
[Midasm Hooks]: ../../how-it-works.md#midasm-hooks
[function discovery]: ../../function-discovery.md
[brute-force scans]: ../../function-discovery.md#brute-force-search-for-missed-code
