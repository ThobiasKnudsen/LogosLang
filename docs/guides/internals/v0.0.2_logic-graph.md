---
title: The Logic Graph
---

# The Logic Graph

A node is exactly two pointers, a **type** and a **value**, sixteen bytes, and a
node's identity is its address. The `type` pointer defines how the `value` is read:
the type's definition specifies the node's layout, how it consumes surrounding
tokens during parsing, and the IR it lowers to.

Operands are not stored inline. A binary `+` is a single cell whose `value` points
at a two-field record (its left and right operands), so the operator is a
higher-level identity describing *how to read its operands* rather than a node that
holds them. Which concrete machine operation runs is resolved from the operator
together with the operand types, not from the `+` node alone.

Execution has one rule: to evaluate a node, read its `type`. If that type is a
function, invoke it on the node's value; otherwise the node is data, read through
its type's layout. Everything runnable is a function, operators and control flow
included. Interpreted code *is* its Logic Graph, and a frozen region JIT-compiled
with Cranelift stays reflectable through the same graph it was compiled from.
