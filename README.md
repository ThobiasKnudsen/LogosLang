# LogosLang

Logos is a systems language built on one idea: the program, its types, its proofs, the compiler, and the rules of the language's own syntax all live in a single structure, the **Logic Graph**, which the language can read and rewrite. There is no line between "the language" and "what is written in it".

What you can download today is the **bootstrap seed**: a small Rust program that turns Logos source into the Logic Graph, interprets it, and compiles the functions you ask it to compile with Cranelift. It is the fixed starting point the rest of the language is being built on, in Logos. It is not the finished language. The design lives in [DESIGN.md](./DESIGN.md), which is the ruling document. This README is the short version.

## The idea in five points

1. **One cell.** Every node in the graph is a *dyad*: a pointer to a type and a pointer to a value, sixteen bytes. The type says how the value is read. Following type pointers always ends at `type`, whose type is itself.
2. **One evaluation rule.** To evaluate a dyad, read its type. If the type is a function, run it on the dyad's value. Otherwise the dyad is data. Operators, field access, and `if` are all functions. Operands arrive unevaluated, so `if` runs only the branch it takes without being a special form.
3. **The parser is in the graph.** Every operator, keyword, and type carries its own parse_rank and its own `constructor`, the code that consumes the tokens around it and builds the node. Defining a new operator is writing a type. There is no separate grammar file.
4. **One pass.** Source becomes graph one token at a time, and each expression runs as soon as it is built. Compile-time evaluation is ordinary interpretation that happens earlier: a function can return a type, and an `if` with a known condition drops the untaken branch before it is even parsed.
5. **Interpreted by default, compiled where you say.** Everything runs as a graph walk until you call `f.compile()`. Then the body is lowered to machine code and the next call jumps. The compiled function stays readable through the graph it came from. No build flags exist: the code decides what compiles.

## A tour of the syntax

Every snippet in this section runs on today's seed. Try them with `logos '…'` or in the REPL.

### Declare, assign, and the hole

```logos
x := i32 5,      # declare x: a type followed by a value
y := x + 1,      # declare y: the type is read from the value
y = 7,           # reassign an existing name
z := i32 ?,      # declare with no value yet: ? is a hole
z = 9,
x + y + z        # 21
```

`:=` declares and `=` reassigns. Writing `=` to a name that was never declared is an error, so a typo cannot create a variable. A name cannot be declared twice while the first is alive: there is no shadowing. `:=` copies, so after `y := x` a write to `y` leaves `x` alone. There are no annotations on the left of `:=`, only a name: `x := i32 5` reads as "x is an i32 holding 5", and `x := i32 ?` as "x is an i32 with nothing in it yet".

### The comma

Newlines are whitespace. The one separator is `,`, and it is required between two steps where the second uses a name the first declares. Between independent expressions it is optional. The same comma separates fields and arguments.

### Numbers

A literal such as `3` is a compile-time rational until context gives it a type: `x + 1` with `x` an `i32` is i32 addition. Two different concrete types never mix silently. Convert by applying the type, so `i32(2.7)` is `2`. `-y` is negation. Integer division by zero does not crash and does not give zero: it saturates, to the type's largest value for a non-negative dividend and its smallest for a negative one, loud enough to notice.

### Functions and scopes

```logos
double := fn (x := i32 ?) -> i32 ( x + x ),
double(21)                                    # 42
```

A parameter list is a record type, each parameter a field with a hole. The return type after `->` is mandatory, `-> void` when there is none. The body is a scope, and a scope's value is its last expression, so `return` is only written to leave early. Any `( … )` is a scope with a value:

```logos
x := ( a := i32 40, a + 2 ),   # x is 42
x
```

Recursion works, and so does compiling it:

```logos
fact := fn (n := i64 ?) -> i64 ( if n <= 1 (1) else (n * fact(n - 1)) ),
fact.compile(),
fact(20)                       # 2432902008176640000
```

### Control flow

```logos
s := i32 0,
for i in 0..5 ( if i % 2 == 0 ( s = s + i ) ),
j := i32 0,
while j < 5 ( s = s + j, j = j + 1 ),
if s > 10 (s) else (0)         # 16
```

`if`, `while`, and `for` are functions that read their own right side. Bodies are always bracketed scopes. A condition can be any expression, bracketed or not.

### Records

```logos
point := type ( instance ( x := i32 ?, y := i32 ? ) ),
p := point (3, 4),
p.x + p.y                      # 7
```

`type` is both the root of every type chain and the keyword that defines one. The per-instance fields live in the `instance ( … )` block; the body's own lines fill the type's slots (`parse_rank`, `associativity`, `constructor`) or declare members of the type itself, read as `point.member`. A type with per-instance fields is a record. A type whose members are all its own is a namespace.

### Types are values

A function can return a type. The call runs during the pass, and the result is an ordinary type value that flows through `:=` and `==`:

```logos
pick := fn (i := i32 ?) -> type ( if (i == 0) (i32) else (f64) ),
a := pick(1) ?,                # a is an f64 with no value yet
a = 2.5,
pick(0) == i32                 # true
```

### Reflection

`.` reads the fields a value's type defines, and a value's type is not one of them. To read the cell itself, ask for the dyad view with `:`, which reads a name's record rather than its value: `x:dyad` is the cell, `x:scope` the scope `x` was declared in.

```logos
x := i32 5,
x:dyad.type == i32 and i32:dyad.type == type    # true: a type's type is the root
```

### Pointers and the heap

```logos
a := alloc i32 40,                     # an owning pointer; alloc inserts `defer free` here
b := own a,                            # move: a is dead from this line on, b owns the memory
inner := ( c := alloc i32 100, c@ ),   # c is freed when its scope closes
b@ + inner - 98                        # 42, then b is freed at program end
```

`@T` is a pointer type, `p@` dereferences, `&x` takes an address. Locals live on the stack and go away with their scope. Heap memory is explicit: `alloc` returns an owning pointer and inserts a visible `defer free` into the scope that owns it. Teardowns run last-in first-out at scope exit. `own` moves ownership by ending the old name at parse time, and `drop x` runs a destructor and ends the name now. Nothing is destroyed behind your back: every teardown is graph structure you can read. The borrow checker that will prove these uses safe is specified but not yet built, so pointers are unchecked today.

### Comments and strings

`# to end of line` is a comment. It is a real node in the graph, not thrown away, so documentation is a graph query rather than a scraping tool. A string is written `«like this»`, with `{expr}` interpolation inside. There is no I/O yet, so a string cannot be printed. A run prints its final value instead.

### Compile

```logos
sum_to := fn (n := i64 ?) -> i64 (
    i := i64 0,
    s := i64 0,
    while i < n (
        s = s + i,
        i = i + 1
    ),
    s
),
sum_to.compile(),
sum_to(1000000)
```

Before `sum_to.compile()` the loop is a graph walk. After it, the same function is machine code. The ruled behaviour is that a compiled function may call an uncompiled one, the call jumping into the interpreter. Today's seed refuses that case instead, so compile callees before their callers.

### Import

```
logos import ./lib.logos                 # run a file top to bottom
logos 'import ./lib.logos, double(21)'   # ...and call something it exposes
```

There is no `main`. The top level is the program, and the file's last value is its result. An imported file runs inside its own scope and sees only the built-in names and its own imports, never the importer's, so it means the same thing wherever it is imported. A file loads once per run, and an import cycle is an error. Today's seed exposes only the names a file marks `pub`, spelled `pub name := …` in the seed. The ruled spelling is `name := pub …`, and the v0.1.0 preview drops gates entirely: every top-level name is visible, and `pub` and `mut` land together after it.

## Defining the language from inside

This is what the first public preview is built to show. An operator is a type with five slots filled: where it binds, which way it associates, how it parses, and what it computes.

```logos
^ := type (
    parse_rank = *.parse_rank + 1,       # binds tighter than *
    associativity = right,
    constructor = fn (tape := parsing_tape ?) -> void (
        tape[0]:dyad.type = ^,                                  # the ^ node, its operand record empty
        tape[0]:dyad.value.operands.append(tape[-1] and tape[1]),
        tape.remove(1),
        tape.remove(-1)
    ),
    code = fn (a := i32 ?, b := i32 ?) -> i32 ( ? )            # the power computation
),
f := fn (x := i32 ?) -> i32 ( x ^ 3 + 1 ),
f.compile(),
f(2)
```

The parser hands every constructor the *parsing tape*, the cells around it: `tape[0]` is its own cell, negative offsets are to its left, positive to its right, and it may read, write, insert, and remove. Assigning a cell's type makes a fresh node of that type with an empty operand record, which the constructor then fills. Precedence is one number per identity, so a new operator slots between any two existing ones by writing its number relative to theirs. `fn` is the shorthand for a type whose parse_rank, associativity, and constructor are the defaults of a call. Everything above runs today except the `code` slot: an operator that is its own operation is the next step, and until it lands a constructor builds a call of an ordinary function, as `tests/fixtures/squared.logos` does.

## What runs today, and what does not

The seed runs:

- the dyad cell, `type` as a value, the dyad view, and type comparison by identity;
- `:=`, `=`, `?`, juxtaposition, and `,`;
- integer, float, and boolean primitives with conversions, and compile-time rationals;
- `if`, `while`, `for`, functions, scopes, recursion, and records;
- types defined with their own parse_rank, associativity, and a constructor written in Logos, run during the parse over the tape;
- functions returning `type`, dependent declarations, and comptime `if`;
- `alloc`, `own`, `drop`, `free`, `defer`, and raw pointers;
- `.compile()` with a deoptimizing JIT;
- `import`, the command line as source, and the REPL.

Specified in DESIGN.md and not yet built:

- **Gates.** `pub`, `mut`, and `lock` as entries on a name's record, one read-or-write rule for visibility, borrowing, and reflection alike.
- **The borrow checker.** Many readers or one writer, checked per place, with lexical lifetimes.
- **Error values.** `T!`, `try`, and `match`. Today every error is a fault that stops the run with one message.
- **The rewriting engine.** Equality saturation over the graph: one engine for compiler optimization, computer algebra, and your own rewrites, driven by cost functions.
- **Proofs as rewrite rules.** `conjecture ( a + a -> 2 * a ) where ( … ) proof ( … )`, checked by a small trusted core. A theorem is a rule with an accepted derivation, an axiom a rule a scope accepts without one.
- **Refinement types** with an SMT solver, plus pre- and postconditions.
- **The standard library, I/O, and foreign calls** over the C ABI.
- **Async tasks** on user-defined executors, with preemption at graph boundaries.
- **Arrays and strings as values you can index and print.**
- **Self-hosting.** Each `native` primitive is replaced by Logos source until the seed is only a bootstrap.

## Practical

### Install

Builds for macOS, Linux, and Windows are on the [downloads page](https://logoslang.dev/download/) and on [the releases page](https://github.com/ThobiasKnudsen/LogosLang/releases). Each archive unpacks to a `bin/logos` that runs in place. There is nothing to install.

### Run

```
logos import ./file.logos       # run a file
logos 'x := i32 5, x * 2'       # run one line of source
logos                           # start the REPL
logos --help
```

Everything after `logos` is one line of Logos source. There are no subcommands and no flags other than `--help`. A parse error prints `file:line:col` with a caret under the spot. Any error exits with status 1.

### The REPL

One expression per line. Values echo, declarations are silent, and a line that fails leaves no trace: its declarations are rolled back, so the name is free to use again. Ctrl-D exits and runs the session's pending teardowns.

### Build from source

```
cargo build --release                                   # target/release/logos
cargo test --release                                    # unit tests and the end-to-end CLI tests
./target/release/logos import examples/answer.logos     # 42
```

Stable Rust is enough. Cranelift is the one heavy dependency. The Rust toolchain is a build dependency, not a runtime one: the binary ships alone.

### Repository layout

- `DESIGN.md` is the ruling document. Where anything else disagrees with it, it wins.
- `language_sketch.logos` illustrates DESIGN.md in code, including parts not yet built.
- `identities/` holds Logos-source specs of the core identities the seed hand-builds: `dyad`, `type`, `fn`, `scope`, `array`, `(`, `proof`.
- `src/` is the seed: the parser, the runtime, the Cranelift lowering, reflection, the node store, the name index, and one file per built-in identity under `src/identities/`.
- `examples/` holds runnable programs, shipped in every release archive.
- `tests/` holds end-to-end tests of the binary and their fixtures.
- `docs/` holds the documentation, one complete tree per `vX.Y.Z`, rendered by the website.
- `CLAUDE_LOG/` holds the session logs of the design work.

### Versions

Versions are named `vX.Y.Z`. **v0.1.0** is the preview, the first public milestone, built to show the language defining an operator from inside itself. **v1.0.0** is the release and the stability promise, with the standard library and verification included. The seed is the Rust program that runs at every version until self-hosting replaces it, never a version itself. The newest tag today is v0.0.4, and v0.1.0 is in progress.

The docs shipped with v0.0.4 predate the vocabulary rulings of August and September 2026. Where they say *synolon*, *hyle*, or `logos` as the root type, read *dyad*, *value*, and `type`. Where they declare with `:`, read `:= T ?`.

### Releasing

Pushing a `vX.Y.Z` tag to `main` builds the archives, creates the GitHub Release, and freezes `docs/vX.Y.Z/`. **A tag is a one-way door.** A ruleset blocks deleting or moving any release tag, so a tag whose release workflow fails burns that version number for good, and the next attempt needs a strictly higher version. Run the gates locally before tagging, never to find out whether they pass:

```
cargo test --release
bash .github/scripts/docs-check.sh validate
bash .github/scripts/docs-check.sh release vX.Y.Z   # the exact check the gate job runs
bash .github/scripts/docs-check.test.sh
```

`main` is protected. It takes a pull request with the required checks, never a direct push.

## Performance

The same loop as in the *Compile* section, written the obvious way in each language, measured on one core of a Ryzen 7 5800U laptop (Linux, July 2026). A tight arithmetic loop is the worst case for any interpreter and the best case for optimizing compilers, so this is the widest the gaps get.

| Runner | ns per iteration | vs C |
|---|---|---|
| C, `gcc -O2` (auto-vectorized) | 0.47 | 1.0x |
| Rust, `rustc -O` (scalar loop) | 0.95 | 2.0x |
| **Logos, after `sum_to.compile()`** | **1.4** | **3.0x** |
| **Logos, interpreted** | **140** | **~300x** |
| Python 3.13 (CPython) | ~170 | ~360x |

The Rust row uses `std::hint::black_box`, because as literally written LLVM replaces the loop with the closed-form formula. Interpreted Logos sits in CPython's class while staying a graph walk over fully reflectable structure. One `.compile()` call puts the same function within about 1.5x of Rust's scalar code. The remaining gap is backend work, loop rotation and vectorization, not language overhead. Absolute numbers vary with hardware; the ratios are the point.

## License and credit

LogosLang is free and open source under the **Apache License 2.0**. You may use, modify, build on, and redistribute it, including commercially, as long as you keep the required notices.

- **Code:** [Apache-2.0](./LICENSE). Section 4 requires keeping the copyright and [`NOTICE`](./NOTICE) attributions and marking any files you change.
- **The name:** "LogosLang" is a trademark, governed separately by [`TRADEMARK.md`](./TRADEMARK.md). Fork the code freely, but a fork that changes the language must use a different name.

Copyright 2026 Thobias Melfjord Knudsen. LogosLang™.
