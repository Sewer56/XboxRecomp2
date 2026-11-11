# Stack Tracking

!!! tip "Works out where ESP and EBP point before each instruction in a function."

    This is important, as it's common for functions to use `esp` and `ebp`
    to access pushed stack parameters and spilled local variables.

!!! note "Where this happens"

    Under `codegen/`:

    - `OpGraph::new` in `optimizations/helpers/op_graph/mod.rs` maps every path.
    - `FunctionStack::new` in `optimizations/helpers/function_stack.rs` follows every path (steps 1 and 2).
    - `StackFrame::step` in `optimizations/helpers/stack_frame.rs` handles one instruction (step 2).
    - `StackFrame::address` in `optimizations/helpers/stack_frame.rs` turns addresses into positions (step 3).

## Step by step

### 1. Start at the entry

Positions are in bytes, relative to ESP when the function starts.<br/>
So at the first instruction, ESP is at 0. EBP is unknown.

### 2. Step through each instruction

We follow every path, noting ESP and EBP before each instruction:

```asm
push esi             ; ESP: 0
mov  eax, [esp+8]    ; ESP: -4, so this reads entry ESP + 4
pop  esi             ; ESP: -4
ret  4               ; ESP: 0
```

These move the positions:

| Instruction                  | Effect                                     |
| ---------------------------- | ------------------------------------------ |
| `push reg/imm/[mem]`         | ESP - 4                                    |
| `pop reg`                    | ESP + 4                                    |
| `add esp, N` or `sub esp, N` | ESP + N or ESP - N                         |
| `lea esp, [esp+N]`           | ESP + N                                    |
| `mov ebp, esp`               | EBP = ESP                                  |
| `mov esp, ebp`               | ESP = EBP                                  |
| `leave`                      | ESP = EBP + 4; tracked EBP becomes unknown |

For anything not in the table:

- Other changes to ESP, like `sub esp, ecx`: we give up.
- Other writes to EBP, like `mov ebp, eax`: we lose track of EBP.
- Writes that could leave EBP pointing into the stack, like
  `lea ebp, [esp+4]`: we give up.
- A `call`: only accepted if [Function Stacks] has an entry for the called
  function.

### 3. Turn addresses into positions

With the positions known, `[esp+N]` and `[ebp+N]` become offsets from the
entry ESP.<br/>
In the example above, `[esp+8]` is entry ESP + 4.

That's [argument 0][Stack Arguments], just above the return address.

!!! warning "We give up on the whole function if we see:"

    - A `call` that [Function Stacks] has no entry for, like `call eax`.
    - Code we couldn't decode.
    - A jump whose targets we don't fully know, like `jmp eax`.
      Switch tables are fine if every case stays in the function.
    - A path that leaves the function without a `ret`, like a tail-call `jmp`.
    - `loop` jumps. The recompiler can't emit them yet.
    - An ESP change we can't follow, like `sub esp, ecx`.
    - Two paths meeting with ESP or EBP in different places.
    - A `ret` with ESP not back where it started.
    - 16-bit addressing.
    - A function with a [`[[midasm_hook]]`][Midasm Hooks].

[Stack Arguments]: stack-arguments.md
[Function Stacks]: function-stacks.md
[Midasm Hooks]: ../../how-it-works.md#midasm-hooks
