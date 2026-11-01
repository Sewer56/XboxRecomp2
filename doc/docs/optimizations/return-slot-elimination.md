# Return Slot Elimination

Doesn't put the return address on the stack for function calls where possible.

!!! note "Where this happens"

    `run` in `codegen/optimizations/eliminate_return_slot.rs`.

## How it works

!!! info "On x86, function calling works by putting the return address on the stack."

`call` pushes it, and `ret` pops it to jump back.

So when the called function starts, the stack looks like this:

| On entry  | Holds          |
| --------- | -------------- |
| `[esp]`   | return address |
| `[esp+4]` | argument 0     |
| `[esp+8]` | argument 1     |

!!! tip "Recompiled code doesn't need this."

    Since compiler will just use your CPU native calling convention.

So if every caller skips those 4 bytes, the arguments move down:

| On entry  | Holds      |
| --------- | ---------- |
| `[esp]`   | argument 0 |
| `[esp+4]` | argument 1 |

**It applies when:**

- We know every caller. See [Known Callers].
- [Stack Tracking] can follow the called function from its start, without calls.
- [Stack Shift] can move its arguments down 4 bytes.

## Before and after

`sub_2EE070` from Shadow the Hedgehog.<br/>
Finds the highest set bit of a positive argument, or -1 for 0.

Generated with Stack Argument Elimination and speed hacks off, so only this
pass changes.

### Recompiled C++

=== "Caller before"

    ```cpp
    // push ecx
    X86_PUSH(ctx, base, ctx.ecx.u32);
    // call 0x002EE070
    ctx.SubEsp(1); //Adjust stack pointer for call instruction
    sub_2EE070(ctx, base);
    ```

=== "Caller after"

    ```cpp
    // push ecx
    X86_PUSH(ctx, base, ctx.ecx.u32);
    // call 0x002EE070
    X86_EXTERN_FUNC(sub_2EE070_fastcall);
    sub_2EE070_fastcall(ctx, base);
    ```

=== "Called function before"

    ```cpp
    X86_PROTECTED_FUNC(sub_2EE070) {
        // mov ecx, [esp+0x04]
        ctx.ecx.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4);
        // ...
    loc_2EE087:
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    }
    ```

=== "Called function after"

    ```cpp
    X86_FUNC_IMPL(sub_2EE070_fastcall) {
        // mov ecx, [esp]
        ctx.ecx.u32 = X86_LOAD_U32(ctx.esp.u32);
        // ...
    loc_2EE087:
        // ret
        return;
    }

    X86_PROTECTED_FUNC(sub_2EE070) {
        ctx.esp.u32 += 0x4;
        sub_2EE070_fastcall(ctx, base);
    }
    ```

    Calls we rewrote go to `sub_2EE070_fastcall`.
    `sub_2EE070` keeps the old layout for every other caller.
    See [Known Callers].

### Host assembly

Clang 21.1.8, x86-64, `-O2`.

The caller is cut off right after the call, so Clang turns it into a `jmp`.<br/>
Flag math is unchanged, so it's cut out.

=== "Caller before"

    ```asm
    mov    ecx, DWORD PTR [rdi+56]       ; guest ESP
    lea    edx, [rcx-4]
    mov    DWORD PTR [rsi+rdx], eax      ; push ecx
    add    ecx, -8                       ; push + return address
    mov    DWORD PTR [rdi+56], ecx       ; update guest ESP
    jmp    sub_2EE070
    ```

=== "Caller after"

    ```asm
    mov    ecx, DWORD PTR [rdi+56]       ; guest ESP
    add    ecx, -4                       ; push only
    mov    DWORD PTR [rsi+rcx], eax      ; push ecx
    mov    DWORD PTR [rdi+56], ecx       ; update guest ESP
    jmp    sub_2EE070_fastcall
    ```

=== "Called function before"

    ```asm
    mov    eax, DWORD PTR [rdi+56]       ; guest ESP
    add    eax, 4
    mov    ecx, DWORD PTR [rsi+rax]      ; load argument 0
    ; ... the loop
    mov    DWORD PTR [rdi+56], eax       ; ret: guest ESP += 4
    ```

=== "Called function after"

    ```asm
    mov    eax, DWORD PTR [rdi+56]       ; guest ESP
    mov    eax, DWORD PTR [rsi+rax]      ; load argument 0
    ; ... the loop
    ```

    `ret` no longer writes guest ESP.

[Stack Argument Elimination]: stack-argument-elimination.md
[Stack Tracking]: helpers/stack-tracking.md
[Stack Shift]: helpers/stack-shift.md
[Known Callers]: helpers/known-callers.md
