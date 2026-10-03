# Q-21: may a program end a name the run starts with, like `i32`?

## Metadata
- **Status:** answered, relayed 2026-09-29 23:04
- **Priority:** 3 (why: one letter; #211's review runs meanwhile, and the answer turns a stand-in into the final behaviour before the merge)
- **Asked:** 2026-09-29 23:05
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#211](https://github.com/ThobiasKnudsen/LogosLang/issues/211), one check for "is this a place" shared by `free`, `move`, `&` and `=`
- **Branch and worktree:** `issue-211-place-check`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-211)
- **Asked by:** logoslang-issue-211-worker (session 02937cca, ended), Opus 5.5
- **Waiting:** nobody is idle; the review of #211's first slice runs meanwhile
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the label question](../history/Q-17_issue-203.md): unrelated

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. `free i32`

Before this week's fix, `free i32` ended `i32` everywhere, even inside imported files:

```logos
free i32, import tests/fixtures/lib_helper.logos, bump(41)   # failed inside lib_helper.logos: "`i32` is dead here"
```

Today it is refused, as a stand-in until you rule. DESIGN does not say: ›`free x` works on any identity‹ says "one verb releases a name whatever its type", and ›Sections, the arche, and effect identities…‹ says the root section holds every built-in name, but neither says whether a program may end one.

- (a) **No.** `i32`, `?`, `alloc` and every other built-in name belong to every file, so `free i32` is an error that says to name it first: `t := i32, free t`.
- (b) **Yes, for this file only.** After `free i32` this file cannot use `i32`, and an imported file still can.

Recommended: (a), because ending a shared name buys nothing a local name (`t := i32`) does not, and (b) needs a separate "dead" mark per file.

**Answer 1:** 
the thing is that i32 should be immutable so you cannot free it, or at least from a "user" scope. or wait its not allowed because things are still referencing it but it would cost alot to state everythig that references i32, or maybe not. it depends on what identity is referencing i32, since its a node in the LG. maybe scope is what references it? i dont know what is best to say is referencing i32 or other commonly used idenitties. so im actually leaning towards b if you can actually free it when nothing is having a borrowed read