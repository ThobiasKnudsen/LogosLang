---
title: Install and run
---

# Install and run

Builds for macOS, Linux, and Windows are on the
[downloads page](https://logoslang.dev/download/), and every release is also on
[GitHub](https://github.com/ThobiasKnudsen/LogosLang/releases). Each archive
unpacks to a `bin/logos` you can run in place; there is nothing to install.

```
curl -fsSL <asset-url> -o logos.tar.gz && tar -xzf logos.tar.gz
./logos-<version>-<os>-<arch>/bin/logos --help
```

## Running code

There is no `main`, no project file, and no build flags. A file's top-level scope
evaluates in order and its tail expression is the file's value:

```
logos file.logos    # run a file top to bottom
logos               # start the REPL, one expression per line, ctrl-d to exit
```

```logos
# answer.logos
double := fn (x : i32) -> i32 ( x + x )
sum := i32 0
for i in 0..7 ( sum = sum + i )
double(sum)         # 42
```

Compilation is directed by the code rather than by the command line. Call
`.compile()` on a function and the next call jumps to installed machine code:

```logos
sum_to := fn (n : i64) -> i64 (
    i := i64 0
    s := i64 0
    while (i < n) (
        s = s + i
        i = i + 1
    )
    s
)
sum_to.compile()
sum_to(1000000)
```

## What this release actually runs

This is the **bootstrap seed**: a Rust program that parses source into the Logic
Graph, interprets it, and lowers explicitly compiled functions to Cranelift. It
is the fixed reference point the rest of the system is built on, not the finished
language. What runs today:

- the synolon cell, `logos` as a first-class value, `.logos` reflection, and
  logos identity comparison;
- declaration and reassignment (`:`, `:=`, `=`), juxtaposition, and `,`;
- integer, float, and boolean primitives with casts, and comptime rationals;
- `if`, `while`, `for`, functions, and scopes;
- `-> logos` functions resolved at comptime, dependent declarations
  (`a : metalogos(2)`), and a comptime `if` that drops untaken branches unparsed;
- the [drop model](../reference/memory): `alloc`, `own`, `drop`, `free`, `defer`;
- `.compile()` and the deoptimizing JIT;
- the REPL, which rolls back a failed line's declarations.

Not in this release, and specified rather than built: the borrow checker,
`mut` and the other read/write gates, refinement logos, the SMT and proof layers,
the rewriting engine, the standard library, I/O, and self-hosting. Pages that
describe those say so where they do. A program's value is printed as a stand-in
for I/O, so a string has nothing to print through yet.

The seed also ships every primitive as `native` — callable machine code, opaque
to reflection. Reflectability is reached incrementally: each primitive becomes
readable as Logic Graph only once it is ported to Logos source, so the promise
that the system reflects its own logic in full is true at the self-hosted end
state, not at this one.
