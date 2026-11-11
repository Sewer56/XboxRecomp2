# Find EBP Frames

Finds functions that use EBP as the frame pointer.<br/>

This is a compliment to the `nonvolatile_stack` speed hack.<br/>
With this hack, the `[ebp+N]` accesses also skip `volatile`.

!!! danger "A midasm hook replacing EBP in a function where EBP is stack pointer can have unintended effects"

    I've ***absolutely no idea*** why you'd do this; and I've never had to do it
    in 10+ years of game modding; but just a FYI.

    Like bruv- you'd break the game anyway- makes no sense lad.

## How it works

!!! info "If you've done your fair share of game hax on x86, you've probably seen something like this"

```asm
push ebp            ; save caller's EBP
mov  ebp, esp       ; EBP now has this function's 'stack frame' (the 'frame pointer')
mov  eax, [ebp+8]   ; argument 0
...
...                 ; sometime later...
...
mov  [ebp-4], eax   ; a local
pop  ebp            ; put the caller's EBP back
ret
```

In such a scenario, where EBP inherits the stack pointer, and is not later
overwritten, we can assume `[ebp+N]` accesses are all stack accesses.<br/>
(`[ebp+reg*N]` stays `volatile`, since the register could point anywhere.)

Thus we can apply the `nonvolatile_stack` speed hack.

**A function counts as an EBP frame when:**

- It starts with `push ebp`, then `mov ebp, esp` or `lea ebp, [esp+N]`.
    - Or with a direct call to an SEH (structured exception handling) prologue
      helper, with no jump or EBP access before it.
- Nothing else writes EBP, except right before a `ret`:
    - `pop ebp` or `leave`.
    - `add ebp, N` or `sub ebp, N`, then one of those.
- It has no instructions we couldn't decode.

!!! note "We assume `call` opcode to be fine."

    Both `cdecl` and `stdcall` preserve EBP across function calls typically.<br/>

    When generating optimised code for what IDA'ese calls `__usercall`; I've
    never seen `ebp` being used as a param.

### Frames set up by a helper

!!! info "Sometimes, stack frames are set up by special helper methods"

    Microsoft's 'Structured Exception Handling' (SEH) comes to mind.

```asm
mov  eax, handler_table
call __SEH_prolog      ; registers the handler and sets EBP for us
mov  eax, [ebp+0x0C]   ; normal frame access from here on
```

We treat a function as one of these helpers when:

- It's one straight run of instructions ending in `ret`.
- Its only EBP write is `mov ebp, esp` or `lea ebp, [esp+N]`.
- The host doesn't replace it.

We then categorise it as a helper like `__SEH_prolog`.

## Before and after

### Recompiled C++

Same as `nonvolatile_stack`.

=== "Before"

    ```cpp
    // mov ecx, [ebp+0x10]
    ctx.ecx.u32 = X86_LOAD_U32(ctx.ebp.u32 + 0x10);
    ```

=== "After"

    ```cpp
    // mov ecx, [ebp+0x10]
    ctx.ecx.u32 = X86_NONVOLATILE_LOAD_U32(ctx.ebp.u32 + 0x10);
    ```

### Results

Shadow the Hedgehog, x86-64, Clang 21.1.8, `-O2`:

| What                          | Change           |
| ----------------------------- | ---------------- |
| `[ebp+N]` accesses made plain | 40,371 of 51,170 |
| Code size                     | −44 KB (−0.37%)  |
