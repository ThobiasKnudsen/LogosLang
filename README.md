# LogosLang
A self-hosting systems language where the compiler, types, proofs, and syntax all live in one structure the language can read and rewrite.

## Install

Builds for macOS, Linux, and Windows are on the [downloads page](https://logoslang.dev/download/) and on [the releases page](https://github.com/ThobiasKnudsen/LogosLang/releases). Each archive unpacks to a `bin/logos` that runs in place; there is nothing to install and there are no build flags.

```
logos import ./file.logos                  # run a file top to bottom (no main)
logos 'import ./file.logos, main(«hi»)'    # …and call one of its pub names
logos                                      # start the REPL
```

Everything after `logos` is one line of Logos source, run by the one pass. `import` loads a file — it runs top to bottom, and the importer reaches only its `pub` names.

This is the bootstrap seed: it parses source into the Logic Graph, interprets it, and lowers explicitly compiled functions to Cranelift. It is not the finished language. [What this release does and does not run](https://logoslang.dev/docs/getting-started/install/) is listed in the docs, alongside the borrow checker, proof layer, rewriting engine, and standard library, which are specified and not yet built.

## Performance

Logos runs source by interpreting the Logic Graph. Calling `.compile()` on a function lowers its body to machine code (Cranelift) and installs it, so the next call jumps instead of walking the body. Compilation is directed in source, never by compiler flags.

```
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
sum_to(1000000000)
```

The same loop, written the obvious way in each language, measured on one core of a Ryzen 7 5800U laptop (Linux, July 2026). A tight arithmetic loop is the worst case for any interpreter and the best case for optimizing compilers, so this is the widest the gaps get:

| Runner | ns per iteration | vs C |
|---|---|---|
| C, `gcc -O2` (auto-vectorized) | 0.47 | 1.0x |
| Rust, `rustc -O` (scalar loop¹) | 0.95 | 2.0x |
| **Logos, after `sum_to.compile()`** | **1.4** | **3.0x** |
| **Logos, interpreted** | **140** | **~300x** |
| Python 3.13 (CPython) | ~170 | ~360x |

¹ Rust as literally written measures near zero: LLVM recognizes the summation and replaces the loop with the closed-form formula. The scalar number uses `std::hint::black_box` so a loop remains to measure.

Reading the table: interpreted Logos sits in CPython's class (slightly ahead on this loop) while staying a graph walk over fully reflectable structure, and one `.compile()` call puts the same function within about 1.5x of Rust's scalar code and 3x of vectorized C. The remaining gap to Rust is loop shape (Cranelift does not yet rotate loops) and the gap to C is vectorization: both are backend work, not language overhead. Absolute numbers vary with hardware; the ratios are the point.

## Releasing

Pushing a `vX.Y.Z` tag to `main` builds the per-OS/arch archives, creates the GitHub Release, and freezes `docs/vX.Y.Z/`.

**A version tag is a one-way door.** The `freeze-release-tags` ruleset blocks deletion *and* update on every `refs/tags/v*`, so a tag cannot be moved or removed once pushed. If the release workflow fails after the tag lands, that version number is spent: the tag stays pinned to the broken commit and the next attempt must use a strictly higher version. The abandoned version's `docs/` folder is frozen too, since `docs-check.sh` refuses any change at or below the newest tag, so its replacement has to be a copy under the new version. v0.0.3 was lost this way, and v0.0.4 shipped the identical tree.

So run the gates locally before tagging, never to find out whether they pass:

```
cargo test --release --manifest-path seed/Cargo.toml
bash .github/scripts/docs-check.sh validate
bash .github/scripts/docs-check.sh release vX.Y.Z   # the exact check the gate job runs
bash .github/scripts/docs-check.test.sh
```

`main` is protected and takes a PR with required checks, never a direct push.

## License and credit

LogosLang is free and open source under the **Apache License 2.0**. You may use, modify, build on, and redistribute it, including commercially, as long as you keep the required notices.

- **Code:** [Apache-2.0](./LICENSE). Section 4 requires keeping the copyright and [`NOTICE`](./NOTICE) attributions and marking any files you change.
- **The name:** "LogosLang" is a trademark, governed separately by [`TRADEMARK.md`](./TRADEMARK.md). Fork the code freely, but a fork that changes the language must use a different name.

Copyright 2026 Thobias Melfjord Knudsen. LogosLang™.
