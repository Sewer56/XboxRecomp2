# Stack Argument Elimination

Passes stack arguments in emulated x86 registers, where it's safe.

Basically `__fastcall` at home.

!!! note "Where this happens"

    Under `codegen/optimizations/eliminate_stack_args/`:

    - `run` in `mod.rs` runs the steps below.
    - `Called::find` in `called.rs` checks each called function (step 1).
    - `caller::rewrite_calls` in `caller.rs` rewrites the calls (step 3).
    - `Called::rewrite` in `called.rs` rewrites each called function (step 4).

## Rationale

!!! info "Typically in x86 you pass arguments via the stack"

    or so is typical of the common `__cdecl` and `__stdcall` calling conventions.

```asm
; caller
push ecx             ; write argument 0
call f

; f
mov  eax, [esp+4]    ; load argument 0 into eax
add  eax, 0x14
ret
```

However, in recompiled code, this can be extra costly, as stack accesses
are routed via `ctx->esp`, which lives behind a pointer and may not be
mapped to a register.

So we try to rewrite the callers and callees to skip this stack traffic,
and pass directly in registers.

!!! info "You may have heard of the term `__fastcall` before"

    Which certainly has GCC and Microsoft variants...
    This is basically the same idea.

**It applies when:**

- We know every caller. See [Known Callers].
- [Stack Tracking] can follow the callee from its start.
- The callee loads arguments right at the start. See [Entry Loads].
- Nothing in the callee could read an argument write we remove.

## Before and after

### Callee

#### Recompiled C++

=== "Caller before"

    ```cpp
    // push 0x00
    X86_PUSH(ctx, base, 0x0);
    // mov ecx, edi
    ctx.ecx.u32 = ctx.edi.u32;
    // call 0x0022D060
    ctx.SubEsp(1); //Adjust stack pointer for call instruction
    sub_22D060(ctx, base);
    ```

=== "Caller after"

    ```cpp
    // mov ecx, edi
    ctx.ecx.u32 = ctx.edi.u32;
    // (rewritten) mov eax, 0x00
    ctx.eax.u32 = 0x0u;
    // call 0x0022D060
    ctx.SubEsp(2); //Adjust stack pointer for call instruction
    X86_EXTERN_FUNC(sub_22D060_fastcall);
    sub_22D060_fastcall(ctx, base);
    ```

=== "Callee before"

    ```cpp
    X86_PROTECTED_FUNC(sub_22D060) {
        // mov eax, [esp+0x04]
        ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4);
        // mov [ecx+0x04], eax
        X86_STORE_U32(ctx.ecx.u32 + 0x4, ctx.eax.u32);
        // ret 0x04
        ctx.esp.u32 += 0x8;  //Adjust esp for return instruction
        return;
    }
    ```

=== "Callee after"

    ```cpp
    X86_FUNC_IMPL(sub_22D060_fastcall) {
        // mov [ecx+0x04], eax
        X86_STORE_U32(ctx.ecx.u32 + 0x4, ctx.eax.u32);
        // ret 0x04
        ctx.esp.u32 += 0x8;  //Adjust esp for return instruction
        return;
    }

    X86_PROTECTED_FUNC(sub_22D060) {
        ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4);
        sub_22D060_fastcall(ctx, base);
    }
    ```

#### Host assembly

Clang 21.1.8, x86-64, `-O2`.

=== "Caller before"

    ```asm
    mov    eax, DWORD PTR [rbx+56]       ; guest ESP
    lea    ecx, [rax-4]
    mov    DWORD PTR [r14+rcx], 0        ; push: store the parameter
    mov    ecx, DWORD PTR [rbx+40]       ; guest EDI
    mov    DWORD PTR [rbx+16], ecx       ; guest ECX
    add    eax, -8
    mov    DWORD PTR [rbx+56], eax       ; update guest ESP
    call   sub_22D060
    ```

=== "Caller after"

    ```asm
    mov    eax, DWORD PTR [rbx+40]       ; guest EDI
    mov    DWORD PTR [rbx+16], eax       ; guest ECX
    mov    DWORD PTR [rbx], 0            ; guest EAX = parameter
    add    DWORD PTR [rbx+56], -8        ; update guest ESP
    call   sub_22D060_fastcall
    ```

=== "Callee before"

    ```asm
    mov    eax, DWORD PTR [rdi+56]       ; guest ESP
    lea    ecx, [rax+4]
    mov    ecx, DWORD PTR [rsi+rcx]      ; load the parameter
    mov    DWORD PTR [rdi], ecx          ; guest EAX
    mov    edx, DWORD PTR [rdi+16]       ; guest ECX
    add    edx, 4
    mov    DWORD PTR [rsi+rdx], ecx      ; object store
    add    eax, 8
    mov    DWORD PTR [rdi+56], eax       ; ret 4
    ```

=== "Callee after"

    ```asm
    mov    eax, DWORD PTR [rdi]          ; guest EAX already holds it
    mov    ecx, DWORD PTR [rdi+16]       ; guest ECX
    add    ecx, 4
    mov    DWORD PTR [rsi+rcx], eax      ; object store
    add    DWORD PTR [rdi+56], 8         ; ret 4
    ```

### Caller

#### Recompiled C++

=== "Caller before"

    ```cpp
    // push ecx
    X86_PUSH(ctx, base, ctx.ecx.u32);
    // call 0x00014D40
    ctx.SubEsp(1); //Adjust stack pointer for call instruction
    sub_14D40(ctx, base);
    // add esp, 0x04
    X86_ADD32(ctx.esp.u32, 4, ctx);
    ```

=== "Caller after"

    ```cpp
    // push ecx
    X86_PUSH(ctx, base, ctx.ecx.u32);
    // (rewritten) mov eax, ecx
    ctx.eax.u32 = ctx.ecx.u32;
    // call 0x00014D40
    X86_EXTERN_FUNC(sub_14D40_fastcall);
    sub_14D40_fastcall(ctx, base);
    // add esp, 0x04
    X86_ADD32(ctx.esp.u32, 4, ctx);
    ```

=== "Callee before"

    ```cpp
    X86_PROTECTED_FUNC(sub_14D40) {
        // mov eax, [esp+0x04]
        ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4);
        // add eax, 0x14
        X86_ADD32(ctx.eax.u32, 20, ctx);
        // ret
        ctx.esp.u32 += 0x4;  //Adjust esp for return instruction
        return;
    }
    ```

=== "Callee after"

    ```cpp
    X86_FUNC_IMPL(sub_14D40_fastcall) {
        // add eax, 0x14
        X86_ADD32(ctx.eax.u32, 20, ctx);
        // ret
        return;
    }

    X86_PROTECTED_FUNC(sub_14D40) {
        ctx.eax.u32 = X86_LOAD_U32(ctx.esp.u32 + 0x4);
        ctx.esp.u32 += 0x4;
        sub_14D40_fastcall(ctx, base);
    }
    ```

#### Host assembly

Clang 21.1.8, x86-64, `-O2`.

=== "Caller before"

    ```asm
    mov    ecx, DWORD PTR [r14+rcx]      ; mov ecx, [eax]
    mov    DWORD PTR [rbx+16], ecx       ; guest ECX
    mov    DWORD PTR [r14+rax], ecx      ; push: store the parameter
    add    eax, -4
    mov    DWORD PTR [rbx+56], eax       ; update guest ESP
    call   sub_14D40
    mov    ecx, DWORD PTR [rbx]          ; guest EAX, the result
    mov    eax, DWORD PTR [rbx+56]       ; guest ESP, for add esp, 4
    ```

=== "Caller after"

    ```asm
    mov    ecx, DWORD PTR [r14+rcx]      ; mov ecx, [eax]
    mov    DWORD PTR [rbx+16], ecx       ; guest ECX
    mov    DWORD PTR [r14+rax], ecx      ; keep the stored parameter
    mov    DWORD PTR [rbx], ecx          ; guest EAX = parameter
    call   sub_14D40_fastcall
    mov    ecx, DWORD PTR [rbx]          ; guest EAX, the result
    mov    eax, DWORD PTR [rbx+56]       ; guest ESP, for add esp, 4
    ```

=== "Callee before"

    ```asm
    mov    eax, DWORD PTR [rdi+56]       ; guest ESP
    add    eax, 4
    mov    ecx, DWORD PTR [rsi+rax]      ; load the parameter
    mov    esi, ecx
    add    esi, 20                       ; add eax, 0x14
    mov    DWORD PTR [rdi], esi          ; guest EAX
    ; ... flags for add eax, 0x14
    mov    DWORD PTR [rdi+56], eax       ; ret
    ```

=== "Callee after"

    ```asm
    mov    eax, DWORD PTR [rdi]          ; guest EAX already holds it
    mov    edx, eax
    add    edx, 20                       ; add eax, 0x14
    mov    DWORD PTR [rdi], edx          ; guest EAX
    ; ... flags for add eax, 0x14
    ```

## Step by step

1. **Check each callee.** See [Stack Arguments] and [Entry Loads].
2. **Check every caller.** See [Known Callers] and [Call Setup].
3. **Rewrite the callers.** Move each argument into its register right before
   the call, and remove the argument writes nothing reads.
4. **Rewrite the callees.** Remove their entry loads. See [Known Callers].

!!! note "Soz, no more in-depth details. Time constraints."

    Hopefully the helper pages are enough.

[Stack Tracking]: helpers/stack-tracking.md
[Stack Arguments]: helpers/stack-arguments.md
[Entry Loads]: helpers/entry-loads.md
[Call Setup]: helpers/call-setup.md
[Known Callers]: helpers/known-callers.md
