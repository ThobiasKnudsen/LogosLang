---
title: Introduction
---

# Introduction

Logos is a self-hosting systems language built on one commitment: **radical
unification**. Programs, types, proofs, compilation rules, the optimizer, the
standard library, and the compiler's own logic all live in one structure the
language can read and rewrite: the **Logic Graph**.

A small Rust seed starts the system; everything beyond is written in Logos and
processed by the seed until it compiles itself. Code is interpreted by default and
a frozen region can be JIT-compiled with Cranelift, staying reflectable through the
graph it came from.

See [declaration and assignment](../reference/operators) for the core syntax, and
[the rewriting engine](../guides/the-rewriting-engine) for how one operation serves
compiler optimization, computer algebra, and your own transforms.
