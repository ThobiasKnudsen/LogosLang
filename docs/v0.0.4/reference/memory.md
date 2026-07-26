---
title: Memory and teardown
---

# Memory and teardown

Locals are scope-bound and their memory is reclaimed with the frame, an
effect-free bulk operation. There is no GC. Reclamation and destruction are
distinct: returning memory that needs no teardown stays implicit, while running
teardown never is.

## The constructor authors the teardown

`alloc T v` returns an owning pointer `@T`. What frees it is never an invisible
scope-end rule. Only the construction site knows how a value was built, which
allocator it came from, and what invariants hold, so **the constructor writes the
teardown and inserts it as reflectable `defer` structure** into the scope where
the result's ownership lands:

```logos
a := alloc i32 40   # inserts `defer free a` into this scope
a@                  # 40
```

`alloc` is only the first instance of the general rule. Any constructor may
insert its paired teardown, so a file-opening constructor inserts its
`defer close`, and a record logos's derived constructor composes the teardowns
its field definitions declare. Deconstruction has too many shapes for one
mechanism to know them all; each constructor knows its own.

`defer` runs LIFO at scope exit, so teardown order reverses construction order.
Because teardown is ordinary graph structure rather than hidden drop glue,
"every `alloc` reaches a paired `free` on every path" is a proposition over the
graph that the proof layer can discharge like any other.

Recorded as rejected, to stay rejected: implicit scope-end destruction in any
form, and `:=` inserting `defer drop` on every declaration. Declaration stays
neutral; teardown authorship belongs to constructors alone.

## `own`, `drop`, and the cancelled defer

`own` and `drop` are **takes**: they consume, emptying the source. A pending
deferred teardown over an emptied place is a sanctioned no-op, which is what
cancels the inserted `defer` on an early `drop` and on transfer of ownership out
of a scope, with no graph edit:

```logos
a := alloc i32 40
b := own a          # ownership moves to b; a's pending free no-ops
b@                  # 40
```

```logos
a := alloc i32 5
v := a@
drop a              # runs a's destructor now; the scope-exit free no-ops
v                   # 5
```

A borrow mints the same `@T` with a null destructor, so owning-ness rides the
node `alloc` built rather than `@T` in general:

```logos
x := i32 3
p := &x
p@                  # 3, and p owns nothing
```

## Three rules that keep ownership from escaping

Each is a place where ownership would otherwise slip past the machinery that
frees it, so each fails closed:

1. **An owning value must be bound to a name**, which is where its `free`
   attaches. `f(alloc i32 5)` is refused rather than leaked; bind it first and
   pass the name.
2. **A scope's value may not be a place that scope owns**, since the inserted
   teardown would free it on the way out and hand back freed memory. `own` is how
   ownership leaves a scope, so it is required.
3. **Ownership may not cross a function return.** A block hands ownership to its
   binder in full view of the parse, but a call hides its body behind a return
   logos that cannot yet say it transfers ownership, so the caller would not know
   it owes a `free`.

The third lifts once a logos can carry its ownership mode, which is the
ownership-gate work: `take` and `drop` as gates on a reference, the same
primitive as `pub` and `mut`.

## Standing approximations

Two pieces of machinery are not built yet, and this release stands in for them:

- The *emptied-to-undefined* flag is a **null pointer** written into the place.
  The seed has no phase bits, so a deferred teardown over a null place is exactly
  the sanctioned no-op.
- Teardown attaches at the **binding site**, so only named owning bindings are
  covered. The attachment rule for a bare owning temporary, and the composition
  order of field-declared teardowns, are both still open.

Shared ownership is a recorded direction rather than built code: a `share`
operator would co-own *without* emptying the source, its destructor decrementing
a reference count and freeing at zero, on the same destructor-slot machinery this
page already describes.

`&T` and `&mut T` are statically checked, many shared XOR one exclusive, with
lexical lifetimes. That checker is specified and not in this release; see
[what this release runs](../getting-started/install).
