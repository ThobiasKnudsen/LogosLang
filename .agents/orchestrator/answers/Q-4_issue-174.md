# Q-4: what should a record print when it is the last line of a program or a REPL line?

## Metadata
- **Status:** answered, relayed 2026-09-29 20:20
- **Priority:** 1 (why: the #174 worker is idle until you answer; one option number answers it)
- **Asked:** 2026-09-29 20:30
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#174 what does a record print?](https://github.com/ThobiasKnudsen/LogosLang/issues/174), also unblocks [#182](https://github.com/ThobiasKnudsen/LogosLang/issues/182)
- **Branch and worktree:** `issue-174-record-print`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174)
- **Asked by:** logoslang-issue-174-worker (session 5d14174f), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`print «…»` is the output word‹, whatever you pick is also what `print «{p}»` shows

## Question
Today, on dev 3f62c33:

```logos
pt := type (a := i32 ?, b := i32 ?), pt(3, 4)
# 140237662005632        the address of scratch memory, different every run

pt := type (a := i32 ?, b := i32 ?), p := pt(3, 4), p
# run error: a record, text or hole is not read as one value

pt := type (a := i32 ?, b := i32 ?), pt
# type                   the type does not print its name either
```

The same record prints an address without a name and is an error with one. DESIGN.md has no rule for this.

## Options
1. **The type's name:** `pt`. You see the kind, not the content. Nested: `line`.
2. **The fields with the type name in front:** `pt(3, 4)`. The output is the Logos that rebuilds the value. Nested: `line(pt(0, 0), pt(3, 4))`. A pointer field prints as a pointer prints today.
3. **The fields alone:** `(3, 4)`. Shorter, but one two-`i32` record looks like any other.
4. **`dyad`**, as a node value prints today. Tells you nothing.
5. **Keep the address.** Changes every run, cannot be tested, and the named `p` still needs a fix.

Also: with option 2 the worker suggests one rule, **"a value prints as the Logos you would write to get it"**: `5` prints `5`, `i32` prints `i32`, `pt(3, 4)` prints `pt(3, 4)`, and #182's `x:frame` prints the function's name, `f`. Say yes or no to that rule also covering #182's named cases. Things with no name (the top-level scope, an unnamed `type (…)`) stay open under #182 either way.

Recommended by the asker: option 2, with one rule covering #182's named cases, because the screen then shows valid Logos that rebuilds the value.

## Answer
<!-- Write below this line and save. Your words go to the agent verbatim. -->
its basically the same as when printing a number. its really a node with an address but since its a number its preinted a specific way and so there should be a share print function in each type to define how something should be printed. 
