---
title: Introduction
---

# Introduction

Logos is a self-hosting systems language built on one commitment: **radical
unification**. Programs, logos, proofs, compilation rules, the optimizer, the
standard library, and the compiler's own logic all live in one structure the
language can read and rewrite: the **Logic Graph**.

## The synolon: one cell, two slots

Every cell in the Logic Graph is a **synolon**, and a synolon is exactly two
slots: a **logos** and a **hyle**. The logos classifies; the hyle is the raw
substance being classified. The logos defines how the hyle is read, so a hyle
always carries a logos, while a logos is an ordinary value that can stand on its
own.

`logos` is one identity, not three. What other languages split into a *type*
(classification), a *struct* (a derived layout), and a *language* or *namespace*
(a bag of definitions) is a single thing here. The difference between those cases
is only which members vary per value: members that can change are per-instance
and give you a record layout, members that never change are stored once with the
logos itself, and a logos whose members are *all* shared is a pure namespace.

```logos
x := i32 5

x.logos == i32       # true: x was built as an i32
i32.logos == logos   # true: a logos's own logos is the `logos` root
```

A logos is a value, so it flows through `:=`, `==`, and function returns like
anything else, and the classifier tower terminates at the `logos : logos`
self-loop.

## One evaluation rule

To evaluate a synolon, read its logos. If that logos is a function, invoke it on
the synolon's hyle; otherwise the synolon is data, read through its logos's
layout. Everything runnable is a function, operators and control flow included.

Operands arrive unevaluated, so control flow needs no special form: `if` receives
its branches as Logic Graph and evaluates only the one it takes.

## Interpreted by default, compiled on request

Code is interpreted directly from the graph, so interpreted code *is* its Logic
Graph. Calling `.compile()` on a function lowers its body to machine code with
Cranelift and installs it, and the next call jumps instead of walking the body.
Compilation is directed in the source, never by a build flag, and a compiled
function stays reflectable through the graph it was compiled from.

## Where to go next

- [Install and run](install) to get the binary and see what this release does.
- [Declaration and assignment](../reference/operators) for the core syntax.
- [Memory and teardown](../reference/memory) for `alloc`, `own`, `drop`, and `defer`.
- [The Logic Graph](../guides/internals/logic-graph) for the representation itself.
- [The rewriting engine](../guides/the-rewriting-engine) for how one operation
  serves compiler optimization, computer algebra, and your own transforms.
