# Q-28: #197: may a body be built partly where it is written, now? And does a `share` line in a function body run at the definition?

## Metadata
- **Status:** answered, relayed 2026-09-30 00:50
- **Priority:** 1 (why: the #197 review waits for it, and #197's code change is next after #220; two letters)
- **Asked:** 2026-09-30 00:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built (your Q-19 answers, being recorded in DESIGN)
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-1 (session 34384c84), Opus 5.5
- **Waiting:** the #197 review waits for this
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [#219](https://github.com/ThobiasKnudsen/LogosLang/issues/219), your idea: build a kept body partly where it is written, and finish the rest when the types arrive
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Two muts, and the storage partition‹ and ›The pass runs only as far as it must, in order, and never twice‹ (question 2)

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. Build a body as far as it can go where it is written: the rule now, or later with #219?

On `sq` (Q-19, point 1) you wrote: "immediate should be runned in place when parsed and then its stops at a*a". That is building a body partly. The record written from it builds the whole `run` body at once, and leaves building partly to #219. The two readings print different things:

```logos
sq3 := type ( a := ?, output_type := type ?,
              share run = ( immediate ( print «hi», 1 ), a * a ),
              share parse = ( …, tape[0].output_type = tape[-1].type, … ) )
print «defined», x := i32 3, x sq3, y := f64 2.0, y sq3
```

- (a) **Partly, now:** prints `hi defined`. The `immediate` needs nothing from a node, so it runs once where it is written; only `a * a` waits for each type of `a`. The seed catches up with #219.
- (b) **Whole body or nothing now, partly later with #219:** prints `defined hi hi`. The whole body waits for each type of `a`. Three smaller points then stay open in DESIGN.

Recommended: (a), because it is what your `sq` words say, and your Q-19 answer 3 ("a `type (…)` in a body is built where it is written") already needs it. Its cost: your first #197 answer had `sq3` print `hi` again for each new type of `a`; under (a) it prints once.

**Answer 1:** 
yes a. but i came to see that when immediate is unable to run because it hit some ? then it should fail. what do you think about that? its an idea not a rule. 

## 2. A `share` line in a function body: does its value get made at the definition or at the first call?

Two DESIGN rules disagree:
- ›Two muts, and the storage partition‹ (25 September, your "agree"): "The initializer runs **once, when the definition is parsed**, not at the first call. … A `print` in it prints at the definition."
- ›The pass runs only as far as it must…‹: "A deferred scope (a function body, a loop body …) is a body: nothing in it runs in the pass, except what `immediate` names". Only `immediate` was added there, on 28 September; `share` was never considered.

```logos
bump := fn () -> i32 ( print «bump», 1 )
f := fn () -> i32 ( share k := bump(), k )
print «defined», f()
```

- (a) **At the definition, as an `immediate` runs:** prints `bump defined 1`, as the seed does today. The pass rule names `share` beside `immediate`. In a loop it matches your Q-19 answer 3: `for 0..3 ( t := type ( share g := bump() ), print «iter» )` prints `bump iter iter iter`.
- (b) **At the first call:** prints `defined bump 1`. The `share` rule changes.

Recommended: (a), because `share` means "one value, made once", and the `share` rule's own reason (its initializer "cannot read the function's parameters or locals") holds only if it runs at the definition.

**Answer 2:** 
a

## 3. Your idea: an `immediate` that hits a `?` fails. What I think

I agree, and I would make it a rule. Today's text (29 September) runs such an `immediate` again each time the body is built for a new type. Your idea gives it one moment only: where it is written.

```logos
sq4 := type ( a := ?, share run = ( n := immediate ( a.type.size_bytes ), a * n ) )
# today's text: runs at each build, so n is 4 for i32 and 8 for f64
# your idea:    an error where it is written: `a` has no type there yet
```

- **For it:** the word then means what you chose it for, "something should be done right now in the moment". A reader knows when it runs without checking whether the body waits for a type. No Logos file uses `immediate` yet, so nothing breaks.
- **What it removes:** `immediate` as a way to work out one value per type inside a body that waits for a type, like `n` above. Without the word, that line reads `a.type.size_bytes` each time it runs. That works in the interpreter. In compiled code a type is "an opaque eight bytes" (›A type is a comptime value…‹), so there the build would have to work it out. That is a separate point, not needed now.

- (a) **Make it a rule now:** the #197 review records it with your answers 1 and 2.
- (b) **Keep it an idea:** the review records it as an Open line, and the 29 September text stands until you decide.

Recommended: (a).

**Answer 3:** 
a