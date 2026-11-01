# Optimization Name

One or two sentences: what this optimization removes or speeds up.

## How it works

Show the guest code it targets, then say in a few sentences what changes.

```asm
; guest instructions this optimization targets
```

**It applies when:**

- Condition one.
- Condition two.

## Before and after

### Recompiled C++

=== "Before"

    ```cpp
    // generated C++ without the optimization
    ```

=== "After"

    ```cpp
    // generated C++ with the optimization
    ```

### Host assembly

Compiler, target and flags, e.g. Clang 21.1.8, x86-64, `-O2`.

=== "Before"

    ```asm
    ; host assembly without the optimization
    ```

=== "After"

    ```asm
    ; host assembly with the optimization
    ```

## In the code

| What           | Where               |
| -------------- | ------------------- |
| Does the work  | `path/to/pass.rs`   |
| Writes the C++ | `path/to/writer.rs` |

All paths are under `src/xbox-recomp2/src/`.
