# LogosLang
A self-hosting systems language where the compiler, types, proofs, and syntax all live in one structure the language can read and rewrite.

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

## License and credit

LogosLang is free and open source under the **Apache License 2.0**. You may use, modify, build on, and redistribute it, including commercially, as long as you keep the required notices.

- **Code:** [Apache-2.0](./LICENSE). Section 4 requires keeping the copyright and [`NOTICE`](./NOTICE) attributions and marking any files you change.
- **The name:** "LogosLang" is a trademark, governed separately by [`TRADEMARK.md`](./TRADEMARK.md). Fork the code freely, but a fork that changes the language must use a different name.

Copyright 2026 Thobias Melfjord Knudsen. LogosLang™.
