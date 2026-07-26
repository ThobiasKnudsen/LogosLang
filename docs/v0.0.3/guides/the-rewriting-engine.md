---
title: The rewriting engine
---

# The rewriting engine

> Specified, not in this release. See
> [what this release runs](../getting-started/install).

Logos collapses three things other languages keep apart (compiler optimization,
computer algebra, and user-defined transformation) into one operation: take a
Logic Graph or Logos IR fragment, apply rewrite rules, and extract the form that
minimizes a cost function.

The engine uses **equality saturation**. Rather than rewriting destructively in
priority order, it builds an e-graph of the forms equivalent to the input under
the rule set, then extracts the lowest-cost one. Extraction is optimal over the
forms the e-graph actually holds; a true fixpoint is not guaranteed, since
size-increasing rules can grow the e-graph without bound, so the engine runs
under an explicit iteration, size, and time budget and extracts the best form
found within it.

Rule sets and cost functions are ordinary, first-class Logos values, so a
compiler optimization pass is a library rather than special compiler code.

```logos
eval(optimize_for(target, kernel))   # target-specific rewriting, at compile time
```

A cost function maps Logic Graph nodes to costs. The standard library is to ship
FLOP count, numerical stability, code size, register pressure, and target-specific
metrics; users supply their own. The different jobs people want from a rewriter
are then just a choice of cost function or rule subset: minimum-cost (the default
`simplify`), canonical (equality testing), factored, expanded, numerically stable,
or target-specific.

The same engine serves the compiler's `x + 0 → x` and the mathematician's
`sin²(θ) + cos²(θ) → 1`.

The freeze that licenses compilation is the same one that licenses this:
equality saturation needs a fixed input, so compiling a region, optimizing it,
and extracting a cheaper proven-equivalent variant are one operation under one
condition, that no live structural writer exists over the region. See
[the Logic Graph](internals/logic-graph) for that boundary.
