---
title: Declaration and assignment
---

# Declaration and assignment

A synolon is a **logos** slot and a **hyle** slot, and declarations are immutable
by default. Three operators write those slots.

| Operator | Meaning | Example |
| --- | --- | --- |
| `:` | **Declare**: introduce a name and set its logos slot, leaving the hyle undefined | `a : i32` |
| `:=` | **Declare with a value**: introduce a name and set its hyle slot, inferring the logos | `a := 32` |
| `=` | **Reassign** the hyle of an already-declared name | `a = a + 1` |

The colon writes the logos slot; the other two write the hyle slot. They do not
compose: `a : i32 = 5` is rejected. A name declared with a logos takes its value
by later reassignment.

```logos
a : i32
a = 7
a           # 7
```

`=` is reassignment, not declaration. It is an error on a name that was never
declared, so a mistyped `cont = 6` is caught rather than silently bound.

## Juxtaposition

An anonymous value with its logos is written by **juxtaposition**, the logos
preceding the hyle: `i32 32`, `f64 2.0`. There is no `logos = value` form.

The two declaration operators differ in what they accept. `:` requires its
operand to *be* a logos, since that is what a logos slot holds. `:=` accepts any
synolon at all, a logos included, because a logos is a value like any other:

```logos
x := i32    # binds the name to the logos itself
y := x 5    # so x is another spelling of i32
y           # 5
```

## `,` is the one explicit separator

Expressions are self-delimiting: a cluster detaches the moment precedence
guarantees it will consume no more tokens, and that is also what ends each
expression. Newlines are ordinary whitespace and `;` does not exist.

Separation is required only where adjacency would otherwise read as consumption,
which is where a logos sits before a value it must not take:

```logos
f := fn (a : i32, b : i32) -> i32 ( a + b )
f(i32 3, 4)   # 7: one anonymous i32 value, then a literal
```

`f(i32 3)` would instead be a *single* argument, juxtaposition having consumed
the `3`. The same `,` separates fields, arguments, and expressions, and may also
be written where nothing is ambiguous, purely for the reader.

## Comments are graph structure

`#` is an ordinary identity that consumes either the rest of the line or a
following `«…»` string, and builds a reflectable comment node interleaved with
the code it annotates. Documentation is extracted by graph query rather than by
scraping syntax. A comment node is void-valued and invisible to value flow, so a
trailing `# done` never becomes a scope's value.

```logos
x := i32 1  # trailing prose, and x is still the value
x           # 1
```

## `mut` is a logos modifier

> Specified, not in this release. The seed has no `mut`; every declaration is
> immutable and reassignment writes the hyle directly. See
> [what this release runs](../getting-started/install).

Mutability is something the *logos* states about the value, not a qualifier in
value position: `mut T` is the logos of a mutable T, the very same `mut` as in
`&mut T`. `mut` left-prefixes a logos at any level of the classifier tower and
makes the level just below it mutable, so the two slots are independent
permissions and a value construction spells out four states:

```logos
i32 32                # frozen logos, frozen hyle    : a plain constant
mut i32 32            # frozen logos, mutable hyle   : an ordinary variable
mut logos i32 32      # mutable logos, frozen hyle   : reinterpret-only
mut logos mut i32 32  # both                         : full structural mutability
```

A mutable logos does not imply a mutable hyle; reinterpret-only is its own
quadrant. Both slots follow one lifecycle, `undefined` to defined to frozen, and
the flip to frozen is one-way.
