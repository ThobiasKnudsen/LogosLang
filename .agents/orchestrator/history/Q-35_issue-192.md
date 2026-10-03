# Q-35: #192: when a program stops on an error, does cleanup run? (and two small approvals)

## Metadata
- **Status:** answered, relayed 2026-09-30 01:52
- **Priority:** 1 (why: question 1 blocks the last slice of #192, the root being built, which #215 and #226 wait for; one letter each)
- **Asked:** 2026-09-30 01:45
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), a value's cleanup (its type's `free`) runs where its life ends; question 1 is [#94](https://github.com/ThobiasKnudsen/LogosLang/issues/94), a run error skips the cleanup
- **Branch and worktree:** `issue-192-teardown-at-life-end`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-192)
- **Asked by:** logoslang-issue-192-worker (session c727a831), Opus 5.5
- **Waiting:** the worker needs question 1 for its last slice; 2 and 3 can wait
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›A checked error is a fault: the task that hit it is cancelled‹, ›Scheduling, preemption, and cancellation happen at boundaries‹, ›Reading a place before its first write is refused at parse‹

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

#192 makes a value's cleanup (its type's `free`) run where its life ends: at a scope's end, at `=`, in an `if`. Before, it was a `defer` written in at the declaration. That part is built and tested.

## 1. A program that stops on an error: does cleanup run?

Today nothing is cleaned up at an error, neither a `defer` nor a `free`:

```logos
box := type ( … share free = ( print «free», … ) … ),
f := fn () -> i32 ( a := box (1, 2), defer print «bye», error «stop» ),
f()
# prints only: run error: stop
```

DESIGN says the pending `defer`s run, "teardown done". It was written in August, when every cleanup was a `defer`. Since 28 September a value's cleanup is no `defer`, so the sentence no longer says whether `free` runs.

- (a) **Everything, like a normal end:** each open scope runs its `defer`s and its values' `free`, last first, only for what was declared before the error. The example prints `bye`, `free`, then `run error: stop`. If a cleanup fails too, the first error is shown and that scope's cleanup stops there.
- (b) **Only the `defer`s:** prints `bye`, then the error.
- (c) **Nothing**, as today.

Recommended: (a). A `free` that prints or closes a file then does the same whether the program ends normally or on an error. The two DESIGN sentences become "each of its live scopes ends as at its normal end (its `defer`s and the `free` of what it holds, last first)".

**Answer 1:** 
a

## 2. May four old sentences be reworded?

They still say cleanup is "inserted" at the declaration:

- ›Scheduling, preemption…‹, rule 1: "…inserted as reflectable structure, the same law as constructor-inserted teardown; the compiler never adds…" loses "the same law as constructor-inserted teardown".
- ›A checkpoint is saved task state‹: "(same law as constructor-inserted teardown and boundary placement)" becomes "(same law as boundary placement)".
- language_sketch.logos: "# be creates; obe's constructor inserts the defer that ends C at scope exit" becomes "# be creates; C's scope ends it at its exit".
- ›A value owns what its elements hold and frees it‹: "runs that free and empties the cell, as `free a` does for an owner" becomes "runs that free and empties the cell, so the free of what holds the cell finds nothing there" (since 28 September a freed name is left `undefined`, not emptied).

- (a) All four as written.
- (b) You give the wording.
- (c) Leave them.

Recommended: (a).

**Answer 2:** 
a

## 3. An owner that may never have been written

```logos
mut a := own @i32 ?,
if c ( a = alloc 1 of i32 5 )
# at the scope's end, `a` holds a block or nothing, depending on `c`
```

The seed frees what `a` holds, and an empty pointer frees nothing, like C's `free(NULL)`. That looks at the bytes while the program runs, and DESIGN says "The run never checks a byte to learn whether a place is filled". But that rule is about reading, and nothing refuses this at parse.

- (a) **Keep it:** freeing an empty owning pointer frees nothing. That is how the pointer's own `free` works, not a check of whether the place was filled.
- (b) **Refuse at parse** an owner that is written only inside an `if` arm or a loop.

Recommended: (a). It is what the seed does today, and nothing changes.

**Answer 3:** 
mut a := own @i32 ?, shouldnt be allowed by the parser because own requires a name. 