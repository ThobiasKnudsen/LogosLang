# Q-9: is a bare parameter `fn (a)` a generic value `a := ?` or the node `a := dyad ?`?

## Metadata
- **Status:** answered, relayed 2026-09-29 20:51
- **Priority:** 3 (why: no session waits; it shapes #198's fix and the generic-function analysis still running)
- **Asked:** 2026-09-29 20:50
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), part of root [#198](https://github.com/ThobiasKnudsen/LogosLang/issues/198)
- **Branch and worktree:** none yet; a worker records your ruling in DESIGN.md after you answer
- **Asked by:** the root-cause analysis `fn-tail-type` and its two reviews, Opus 5.5
- **Waiting:** no session waits
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the analysis](file:///home/o/.claude/orchestrate/rca/logoslang/fn-tail-type.md)
  - [the other open question](../answers/Q-8_issue-197.md): unrelated, answer in any order

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. What a bare parameter means

```logos
f := fn (a) -> i64 ( a )
f(i32 5)
```

DESIGN has two field kinds that could fit a parameter written without a type. `a := ?` takes the argument's value at its own type, and the body is built once per set of argument types, so `f(i32 5)` is checked against `-> i64` when that body is built. `a := dyad ?` takes the argument's node, unrun.

- (a) **`a := ?`**, a generic function, checked per call. This is what the seed already assumes for `fn (a)`.
- (b) **`a := dyad ?`**, the node; then `-> i64 ( a )` is refused where the function is defined, because a node is not an `i64`.

Recommended by the analysis: (a), because the seed already treats `fn (a)` as `fn (a := ?)` and DESIGN says "accepting `?` and holding is what a generic function is".

**Answer 1:** 
a should be of tyoe dyad since that is the only way to have dynamic types but that also means this function cannot be compiled, or it can actually be compiled but it needs specific compilation for each callee since the type may vary. the solution may be to do some sort of minting per combination of types, but that should become its own issue for later and be something that needs to be discussed later. 
