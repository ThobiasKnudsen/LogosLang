# Q-31: #197: `share e := t` in a type inside a function, reading the call's `t`: an error, or made at the call?

## Metadata
- **Status:** answered, relayed 2026-09-30 01:34
- **Priority:** 1 (why: the last open point of the #197 review, whose merge the next builder waits for; one letter)
- **Asked:** 2026-09-30 01:22
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-1 (session 34384c84), Opus 5.5
- **Waiting:** the #197 review waits for this
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-29](../history/Q-29_issue-197.md): your `mk2` comment, "error because t isnt defined in share e := t"
  - [the `share` question](../history/Q-30_share-read-before-write.md): whether a `share` line needs a value; under (b) below, `share e := t` has one, made when the type is built
  - [array.logos](file:///home/o/Personal/Code/LogosLang/identities/array.logos), lines 8 to 12: array's chooser
  - [#231](https://github.com/ThobiasKnudsen/LogosLang/issues/231), your idea that a `-> type ( … )` body is itself the type's scope

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. `share e := t`, reading the function's parameter: an error, or made at the call?

Under Q-29's `mk2` you wrote: "error because t isnt defined in share e := t". But `array i32` gets its type exactly this way today, in array's chooser:

```logos
share get_mint := fn (t := type ?) -> type (
    mut mint := array_mints[t],
    if mint != ? return mint,
    mint = type (
        share element_type := t,
        …
```

DESIGN's own example does the same (›A `type (…)` inside a body is built once per set of the values it reads‹):

```logos
mk := fn (t := type ?) -> type ( type ( share e := t ) )
print «defined», a := mk(i32), a.e == i32
# (a) error where mk is written: `t` has no value there yet
# (b) defined true: the type waits for the call, and its `share e` is made when the type is built, with that call's `t`
```

- (a) **An error:** a `share` may read only what is known where it is written, as an `immediate` does. Array's chooser then has to be written another way (perhaps your #231 idea, which is not designed yet), and the `mk` example goes.
- (b) **Made at the call:** a `share` in a type is made when that type is built, and a type that needs the call is built at the call. Array's chooser and the `mk` example stay. Only `immediate t` stays the error (your answer 1).

Recommended: (b). It keeps array's chooser working and keeps the type templates you chose on 29 September ("i would lean towards b since that allows for more general templates for types"). It also matches your answer 2, where a type inside a function follows a `t` from outside.

**Answer 1:** 
b