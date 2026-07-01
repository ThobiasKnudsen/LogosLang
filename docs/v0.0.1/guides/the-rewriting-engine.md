---
title: The rewriting engine
---

# The rewriting engine

Logos collapses three things other languages keep apart (compiler optimization,
computer algebra, and user-defined transformation) into one operation: take a
fragment, apply rewrite rules, and extract the form that minimizes a cost function.

The engine uses **equality saturation** over an e-graph. Rule sets and cost
functions are ordinary, first-class Logos values, so a compiler optimization pass is
a library, not special compiler code.

```logos
eval(optimize_for(target, kernel))   # target-specific rewriting, at compile time
```

The same engine serves the compiler's `x + 0 → x` and the mathematician's
`sin²(θ) + cos²(θ) → 1`.
