---
title: Declaration and assignment
---

# Declaration and assignment

A node is two slots, a **type** slot and a **value** slot, and Logos construction
is immutable by default. Three operators write those slots.

| Operator | Meaning | Example |
| --- | --- | --- |
| `:` | **Declare**: introduce a name and set its type slot, leaving the value undefined | `a : i32` |
| `:=` | **Declare with a value**: introduce a name and set its value slot, inferring the type | `a := 32` |
| `=` | **Reassign** the value of an already-declared name | `a = a + 1` |

`:` writes the type slot; `:=` and `=` write the value slot. A bare `a :` declares a
name whose two slots are both undefined. There is no `type = value` form.

## `mut` is a type modifier

`mut` is not a qualifier on a variable; it is a modifier on a **type** (`mut T` is
"the type of a mutable T"), the same `mut` as in `&mut T`. The two slots are
independent permissions, so a value construction spells out four states:

```logos
i32 32              # frozen type, frozen value      : a plain constant
mut i32 32          # frozen type, mutable value     : an ordinary variable
mut type i32 32     # mutable type, frozen value     : reinterpret-only
mut type mut i32 32 # mutable type, mutable value    : full structural mutability
```

So an ordinary mutable variable is declared `a := mut i32 32`, and `a = a + 1`
reassigns it.
