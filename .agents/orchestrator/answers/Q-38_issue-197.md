# Q-38: #197: when does a `type (…)` inside a function make its `share` values? Two cases where DESIGN now says both

## Metadata
- **Status:** answered, relayed 2026-10-01 00:21
- **Priority:** 0 (why: the last open points of the #197 review, whose merge the next builder, #197's seed change, waits for; one letter each. 0 so it sorts above file 01, which I did not renumber, since you may have it open)
- **Asked:** 2026-09-30 02:08
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-2 (session b435f3ec), Opus 5.5
- **Waiting:** the #197 review waits for this
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-28](../answers/Q-28_issue-197.md), [Q-29](../answers/Q-29_issue-197.md), [Q-31](../answers/Q-31_issue-197.md): the answers recorded on #197's branch
  - [#197 comment 5901314999](https://github.com/ThobiasKnudsen/LogosLang/issues/197#issuecomment-5901314999): the rest of review round 2, fixed

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

Review round 2 found two programs where the rules recorded from your answers give different outputs. Both are about when a type's `share` line is made. The seed builds such a type at every call today, so either answer needs #197's own seed change.

## 1. A type in a function reads an outer name that already holds its value: built where it is written, or at the first call?

```logos
bump := fn () -> i32 ( print «bump», 1 ),
mut t := type ?,
t = i32,
f := fn () -> type ( type ( share e := t, share g := bump() ) ),
print «defined», a := f()
```

- (a) **Where it is written**, because `t` already holds `i32` there: prints `bump`, then `defined`. After `t = f64`, the next `f()` builds a new type, so `f` still follows `t`. This is your Q-19 answer 3 ("built where written, since everything is known").
- (b) **At the first call**, because a function reads an outer name when it runs: prints `defined`, then `bump`. This is how the line recording your Q-29 answer 2 is worded ("f depends on t existing and when t changes the output of f also changes").

Recommended: (a). It keeps one rule for every body: built where it is written when all it needs is known there. `f` follows `t` either way. The cost: a function that is never called still builds its type, and runs `bump`, where it is defined.

**Answer 1:**
a 

## 2. A type that waits for the call: is a `share` line that needs nothing from the call made once, or once per type?

```logos
bump := fn () -> i32 ( print «bump», 1 ),
mk3 := fn (t := type ?) -> type ( type ( share e := t, share g := bump() ) ),
print «defined», a := mk3(i32), b := mk3(f64)
```

- (a) **Once, where it is written:** prints `bump`, then `defined`. Every type `mk3` makes gets that one value in `g`. This follows "built in part where it is written" (your Q-29 answer 1), the way a `share` line in a function body is made at the definition (your Q-28 answer 2).
- (b) **Once per type, when the call builds it:** prints `defined`, `bump`, `bump`. Each type `mk3` makes has its own `g`. This is your Q-31 "b" read in general, and what the seed does today.

Recommended: (b). A `share` is a place that belongs to one type, and in `mk3` no type exists yet where it is written, since the type needs `t`. Under (b), `immediate bump()` in such a type runs once where it is written, while `share g := bump()` runs once per type. Your open idea "share could replace immediate" would revisit that.

**Answer 2:** 
the general rule here is that the body of a function and type.run always runs per call of the function so option b. you could also have only ( t ) as the body of the function and it would be the same thing as the example you made here 