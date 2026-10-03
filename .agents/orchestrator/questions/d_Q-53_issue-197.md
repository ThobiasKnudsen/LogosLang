# Q-53: Since 1 October a `type (…)` in a body is new at every run. Does that also hold for a call that returns a type, and for a `fn` written in a body?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. A call that returns a type, made inside a body: run once where it is written, or at every run?

Two rules disagree:
- ›Inside a body, a `-> type` call may take an argument known only at run‹ (25 September, your "yes"): "A `-> type` call whose arguments are types or literals runs in the pass, and its type stands in for the call."
- ›A `type (…)` inside a body is built each time it runs‹ (1 October, Q-44 and Q-46): "a new type each time, as every other expression of the body is run again each time the body runs". Its Rejected line: a type "built once where it is written and the same type at every run" is "a mint that nothing in the body writes out".

```logos
bump := fn () -> i32 ( print «bump», 1 ),
mk3 := fn (t := type ?) -> type ( type ( share e := t, share g := bump() ) ),
h := fn () -> type ( mk3(i32) ),
print «defined», a := h(), b := h(), a == b
# today: bump defined true. mk3(i32) runs once, when h is defined
```

With the same type written out by hand in `h`, it prints `defined bump bump false`.

- (a) **The 25 September rule stands.** A call whose arguments are types or literals runs once where it is written: `bump defined true`. Cost: calling `mk3(i32)` and writing its type out by hand give different results, and the call makes a type once although nothing in the body says so.
- (b) **The call runs with the body, at every run**, like the type written out: `defined bump bump false`. A body that wants the type made once writes `immediate`, which works on the branch today:
  ```logos
  h := fn () -> type ( immediate mk3(i32) ),            # bump defined true
  h := fn () -> i32 ( x := (immediate mk3(i32)) ?, 1 ),  # bump defined 1
  ```
  Cost: a body that declares a name of a computed type with no `immediate`, `x := mk3(i32) ?`, becomes an error ("known only when the program runs, and this needs it now"), as `x := type ( e := i32 ? ) ?` in a body already is.

The reviewer recommends (a): the call's arguments are written in the source, so running it where it is written is what `immediate` does. I recommend (b): it is your one rule for every body, and the once-case keeps a way to be written, so the body says it.

**Answer 1:** 

## 2. A function written inside another function's body: one function, or a new one at every run?

Two rules disagree:
- ›`fn` is not a primitive: a function is a type in this same shape‹: "`fn` is shorthand for what `^` spells out". So by the 1 October rule a `fn (…)` written in a body is a new one at every run.
- ›The pass runs only as far as it must, in order, and never twice‹, your Q-28 line: "a `share` initializer in a deferred body runs in the pass too, at the definition", and the `fn` rule's "exactly one specialization, built at definition". So it is one function.

```logos
outer := fn () -> i32 ( inc := fn () -> i32 ( share mut n := i32 0, n = n + 1, n ), inc() ),
a := outer(), b := outer(), print «{outer()}»
# today: 3. One inc, and its n counts across the three calls of outer
```

- (a) **One function, made where it is written:** `3`. A `fn` is then the one exception to "a type in a body is new at every run".
- (b) **A new function at every run of the outer body:** `1`. Its `share mut n` starts over at each call of `outer`. This is the shape a closure would take later (DESIGN has no closure rule yet). The seed would lag behind until #219 (building a body in part where it is written) or a new issue.

Recommended, by the reviewer and me: (b). It follows your Q-44 answers ("the share is a different one for each new type"; "since its inside a run body … the type would be constructed every time the function runs") and Q-46's "one rule for every body".

**Answer 2:** 

## Metadata
- **Status:** open
- **Priority:** two pairs of rules in DESIGN disagree since 1 October, and an agent reading them can be misled; two letters
- **Asked:** 2026-10-01 03:25
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-4 (session 8467695c), Opus 5.5
- **Waiting:** nothing: #197 merges with its text as it stands, and I write your answer into DESIGN.md on `dev` (your Q-51 answer)
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [2610010236_logoslang.md](file:///home/o/.claude/orchestrate/history/2610010236_logoslang.md): Q-46, the 1 October rule both points follow from
