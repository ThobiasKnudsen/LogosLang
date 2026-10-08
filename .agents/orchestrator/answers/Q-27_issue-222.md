# Q-27: in a plain-data type, what does a `?` field hold?

## Metadata
- **Status:** answered, relayed 2026-09-30 00:52
- **Priority:** 3 (why: nobody waits; the seed change is filed from your answer; one letter plus two short sub-answers if you pick A)
- **Asked:** 2026-09-30 00:11
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#222](https://github.com/ThobiasKnudsen/LogosLang/issues/222), what a `?` field holds in a type with no `run` (a record of plain data)
- **Branch and worktree:** none
- **Asked by:** filed as #222 by logoslang-rca-record-generic-field-review-2 (session 02302837, ended), Opus 5.5
- **Waiting:** nobody
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the `i32 m` points](../answers/Q-20_issue-212.md): unrelated

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. `pt := type ( a := ? )`: what does `pt(i32 1)` make?

Today it is refused at parse:

```logos
pt := type ( a := ? ), p := pt(i32 1), 0          # error: this operator cannot compute over these operands
pt := type ( a := i32 ?, b := ? ), p := pt(1, 2)  # the same
```

Elsewhere a `?` already has an answer. As a function's parameter it takes each call's type (one build per set of argument types), and as the field of a type whose nodes run it takes each node's operand type. For a type of plain data, two DESIGN rules meet. A field is "typed per node", but a data type has "one offset per declaration", and "every node is born with its type and its full size", so a `pt` value cannot be made first and have `a` sized later.

- (a) **One layout per set of field types,** as a function keeps one build per set of argument types and `array` one per element type. `pt(i32 1)` and `pt(1/2)` are values of two layouts of `pt`. Example: `pt := type ( a := ?, b := i32 ? ), p := pt(i32 1, 2), q := pt(1/2, 3), print «{p.a} {q.a} {q.b}»` prints `1 1/2 3`.
- (b) **The first value fixes it for the type:** after `pt(i32 1)`, `a` is `i32` everywhere, and `pt(1/2)` is refused. What a later line means then depends on which value came first.
- (c) **`?` is refused in a type without a `run`:** write the field's type, or `dyad ?` for a field that holds any value (#216, the `dyad ?` placeholder). A special case for data types, where your reason for `?` was "i dont want any special cases".
- (d) **An eight-byte box,** what the seed already sizes it as, and what a function's `?` parameter is today.

Recommended: (a), because `?` then stays one form "for a name, a field and a parameter alike", and a data type is "a type in this same shape" as a function, without the `run`.

**Answer 1:** 
a but i want to clear up that 1/2 get evaluated to rational_number 0.5

## 2. Only if you pick (a): what do `.type` and `size_bytes` answer?

- A1: `pt(i32 1).type` is `pt`, or the layout made for `i32` (as `array i32 [1, 2]`'s type is `array i32`)?
- A2: `pt.size_bytes` for `pt` itself, which has no single size: an error, or something else?

**Answer 2:** 
i would go for A1 here here is something A1 doesnt cover: size_bytes should be defined as share inside pt and look at all the size of the non share fields inside itself (which is pt in this case) of that mint and sum it together and also thinking about allignment. im not 100% sure this would be correct but im quite sure 

## 3. To be sure of A1: what is `pt(i32 1).type`?

A1 held two answers, so "A1" could mean either one:

```logos
pt := type ( a := ? ), p := pt(i32 1), q := pt(1/2)
p.type == q.type    # (a) true   (b) false
```

- (a) **`pt`**, the same for every value of `pt`.
- (b) **The mint for `i32`**, as `array i32 [1, 2]`'s type is `array i32`. Then `p.type.size_bytes` is that mint's size, as your `size_bytes` answer describes.

Recommended: (b), because your `size_bytes` answer sums the fields "of that mint", and under (a) a value's `.type` would not say which mint it is.

**Answer 3:** 
ohh i see now. it would actually be the same case as for array. it should create minted types of pt so its actually false. 