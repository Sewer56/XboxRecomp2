# Needed Flags

Finds which EFLAGS must be valid after each instruction.  
Sometimes referred as 'liveness analysis' by compilers out there.

!!! note "Where this happens"

    `FlagGraph::walk` in `codegen/optimizations/helpers/needed_flags/graph.rs`.

## Rationale

!!! info "Some optimizations need to know if a later instruction can still read a flag"

```asm
add eax, ebx    ; sets ZF, but it's unused...
cmp eax, 0x20   ; because this cmp overwrites it
je  equal       ; reads ZF
```

Now, `clang` is probably smart enough to figure out this one, since a value
is written twice, without a read. *However* there's some cases where clang might
not be able us:

```asm
add eax, ebx    ; eflags changes
call sub_A
```

In such a case, `clang` can't know whether `sub_A` reads the flags or not.

!!! tip "Usually they're not"

Now, generally compilers, including MSVC and GCC typically assume `eflags` to be
'clobbered' (i.e. not preserved) across calls. However, because we pass the
`X86Context&` around

```cpp
void sub_A(X86Context& ctx) 
{
    // ctx inherits from caller
}
```

`clang` can't optimise around the fact that `sub_A` doesn't need the `eflags`.

Many other cases like this also exists.

!!! note "Special Exceptions"

    Handrolled asm that's used for error handling could assume `eflags`
    isn't clobbered.

## How it works

!!! tip "`FlagGraph::walk` starts at the end of the function and works up."

For each instruction, in this order:

1. **Needed after:** flags needed before anything that can run next.
2. **Needed before:** needed after, minus flags it writes, plus flags it reads.
3. **Recheck:** if the flags needed before changed, check whatever can run
   before it again.

Each instruction's flag reads and write info comes from [Flag Effects].

```asm
add eax, ebx
je  skip
adc ecx, 0
skip:
sets dl
ret
```

It's hard to explain how this works; so it's best to show by example.

**Step 1: `ret`**

```asm
ret             ; before: none     <- callers assume no eflags
```

Nothing in the function runs after `ret`.<br/>
No flags needed.

**Step 2: `sets`**

```asm
sets dl         ; before: SF       <- writes(none), reads(SF)
                ; after:  none     <- what ret needs
ret
```

(`sets dl` stores 1 in `dl` when SF is 1.)

**Step 3: `adc`**

```asm
adc ecx, 0      ; before: CF       <- writes(all), reads(CF)
                ; after:  SF       <- what sets needs
skip:
sets dl
```

As a reminder:

`before = after - writes + reads`.<br/>
`before = SF - ALL + CF`.<br/>
Hence it's CF.

**Step 4: `je`**

```asm
je  skip        ; before: ZF, CF, SF  <- writes(none), reads(ZF)
                ; after:  CF, SF      <- adc needs CF, sets needs SF
adc ecx, 0
skip:
sets dl
```

`je` can go two ways, so:

`after = before(adc) + before(sets)`.<br/>
`after = CF + SF`.<br/>
`before = after - writes + reads`.<br/>
`before = CF + SF - NONE + ZF`.<br/>
Hence it's ZF, CF, SF.

**Step 5: `add`**

```asm
add eax, ebx    ; before: none        <- writes(all), reads(none)
                ; after:  ZF, CF, SF  <- what je needs
je  skip
```

Here, `before` is none because we write (remove) all, read (add) none.

But, we can see, that from the result of `add`, we must leave ZF, CF, SF valid
for the next instructions. Others we can discard.

This is where that magical speedup comes from 😉

Clang can optimise some of it, but not all of it; the rest, comes to us.

### Calls & Hooks

!!! info "These don't use [Flag Effects]."

A `call` uses analysis from [Flag Boundaries]:

```asm
add  eax, ebx   ; after: CF     <- sub_A reads CF
call sub_A      ; summary: reads CF, keeps none
```

A [midasm hook] gets the whole `ctx`, so it needs every flag.

## Results

There are two ways to run this.  

They run effectively the same analysis, but are suited for slightly different
use cases.

### `FlagGraph::walk`

You give it two things a function can't work out alone:

- Which flags callers need after `ret`.
- What each function it calls does with flags.

Each function's answers depend on other functions' answers.  
[Flag Boundaries] reruns it until they stop changing.

You get the flags needed before and after each instruction:

```rust
Solution {
    // You should recognise these results, look above!
    live_before: vec![
        none,         // 0: add eax, ebx
        ZF | CF | SF, // 1: je  skip
        CF,           // 2: adc ecx, 0
                      // skip:
        SF,           // 3: sets dl
        none,         // 4: ret
    ],
    live_after: vec![
        ZF | CF | SF, // 0: add eax, ebx
        CF | SF,      // 1: je  skip
        SF,           // 2: adc ecx, 0
                      // skip:
        none,         // 3: sets dl
        none,         // 4: ret
    ],
}
```

### `NeededFlags::new`

Runs `walk` once, with the answers from [Flag Boundaries].

```rust
let boundaries = FlagBoundaries::build(models, graph, hack, diag);
let needed = NeededFlags::new(&models[0], &boundaries);
```

You get:

- `live_after`: same as above.
  [Skip Unused Flags] uses it to drop flags nothing reads.
- `live_on_edges`: what each path out of an instruction needs.
  A jump's two paths can need different flags.
  [Merged Compare and Jump] uses this to write flags only on the path that
  needs them.

```rust
NeededFlags {
    live_after, // <- same as in Solution

    // One list per instruction.
    live_on_edges: [
        [ZF | CF | SF], // 0: add  -> je
        [SF, CF],       // 1: je   -> sets (taken), adc (not taken)
        [SF],           // 2: adc  -> sets
                        // skip:
        [none],         // 3: sets -> ret
        [],             // 4: ret  <- no entries
    ],
}
```

To read an instruction's flags, call `on_edges`:

```rust
let flags = needed.on_edges(1); // 1: je -> [SF, CF]
```

Each instruction has one entry per way it continues:

- Conditional jump: the target, then the next instruction.
- Switch: cases inside the function, then cases leaving it.
- `ret`: none.
- Anything else: one entry.

[Flag Effects]: flag-effects.md
[Flag Boundaries]: flag-boundaries.md
[Skip Unused Flags]: ../skip-unused-flags.md
[Merged Compare and Jump]: ../merge-compare-and-jump.md
[midasm hook]: ../../how-it-works.md#midasm-hooks
