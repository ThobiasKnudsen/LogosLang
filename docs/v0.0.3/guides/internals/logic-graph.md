---
title: The Logic Graph
---

# The Logic Graph

A synolon is exactly two pointers, a **logos** and a **hyle**, sixteen bytes, and
its identity is its address rather than a stored field. The logos defines how the
hyle is read: it specifies the synolon's layout, how it consumes surrounding
tokens during parsing, and the IR it lowers to.

Operands are not stored inline. A binary `+` is a single cell whose hyle points at
a two-field operand record, so the operator is a higher-level identity describing
*how to read its operands* rather than a cell that holds them. Which concrete
machine operation runs is resolved from the operator together with the operand
logos, not from the `+` cell alone.

## One evaluation rule

To evaluate a synolon, read its logos. If the logos is a function, invoke it on
the synolon's hyle; otherwise the synolon is data, read through its logos's
layout. Everything runnable is a function, operators and control flow included.

Control flow works as a function precisely because operands arrive
*unevaluated*: a function receives its operand record as Logic Graph and decides
what to evaluate, so `if` runs its condition and then only the taken branch.
Laziness is what the substrate gives by default, not a special form.

## A logos's metadata is shared; its per-value data is not

A logos stores its `precedence`, `associativity`, `constructor`, and `destructor`
once and shares them across every value of that logos. The bytes that differ per
value are the per-instance fields.

Placement is decided by mutability rather than annotation. A member that can ever
change is per-instance, and a member that never changes is identical across every
instance and stored once with the logos. That single partition is why the record
case and the authored case are one identity: a logos whose constructor derives its
layout from the field declarations in its scope is what other languages call a
struct, a logos whose members are all shared is a namespace, and neither needs a
second mechanism.

## Uniform model, non-uniform storage

Every identity *has* a permission and a lifetime in the model; almost none *store*
one. The cell stays lean at its two slots, metadata hangs off only on deviation,
and defaults are inherited from the scope tree. The common case, immutable
`'static` definitions such as operators, logos, and the standard library, shares
one default of readable-by-all with no lifetime tracking, no drop node, and no
borrow state. Storage is proportional to *deviations*, not to identity count,
which is what makes a uniform model affordable.

## Parsing is the constructors driving a tape

There is no parse-then-run step. Source is converted to synolons one token at a
time and the graph is interpreted directly.

Two states stand at every source index: the **parsing tape** and the growing
Logic Graph. The tape is the working frontier, holding both the synolons reduced
so far but not yet final and the tokens still to consume, the latter lexed lazily
on demand, so lexing and parsing are one pass that the constructors drive rather
than two.

Each logos's `constructor` reads forward on the tape for the tokens it consumes
and back on the tape for its left context. When precedence guarantees a cluster
will consume no more tokens it is final, so it detaches from the tape and attaches
at the current location in the graph. A constructor edits the tape *in place*, and
the driver decides only *when* constructors run, never what they leave.

The tape carries `insert` and `remove`, and that is the whole macro and
custom-syntax mechanism: a syntactic abstraction is a constructor rewriting the
token stream over real synolons, not a macro sublanguage. The tape is therefore
not a private parser structure but Logic Graph with a string extension, since a
reduced cell is a graph synolon already and a token cell is a synolon-in-waiting.
Reflecting on it needs no second metalanguage.

## Compilation is a reader of frozen structure

Interpreted code *is* its Logic Graph. A compiled artifact is a long-lived
*reader* of a subgraph's structure, and what licenses it is the absence of any
structural writer over that region for the artifact's lifetime.

So the rule that a graph mutation invalidates outstanding readers *is*
deoptimization: a structural write to compiled code drops the artifact and falls
back to interpretation, while ordinary runtime values flow through unaffected,
because what froze is the code's structure and not its data. This is also why a
JIT-compiled region stays fully reflectable through the graph it was compiled
from.

This release does not promote automatically. It interprets everything and
compiles only what the source directs with `f.compile()`; scope-granular backend
markers and profile-driven promotion are later work riding the same deopt
boundary.
