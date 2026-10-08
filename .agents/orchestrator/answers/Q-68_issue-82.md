# Q-68: may an `if` at a function's end have an arm that leaves the function?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

The #82 worker found that I recorded your Q-63 answers 3 and 4 wider than you wrote them:
- you wrote "each if-else branch in an assignment must return something to the assignment operation";
- I wrote "every arm of an `if` whose value is used".

The two differ at the end of a function, where the `if`'s value is the function's result. That is not an assignment:

```logos
f := fn (x := i32 ?) -> i32 ( if (x > 2) (error «too big: {x}») else (x) ),   # runs today; two tests use it
g := fn (c := bool ?) -> i32 ( if c (return 1) else (i32 2) ),                # runs today
```

- (a) **Every `if` whose value is used**, a function's end included. `f` and `g` are refused, and `f` is written `if (x > 2) (error «too big: {x}»), x` instead.
- (b) **An arm may leave the function only where the `if` is the function's end.** Here, `return 1` hands its value to the same call that the `else` arm's value goes to, so nothing is lost and nothing is ambiguous. Everywhere else every arm must give a value, as in an assignment. `f` and `g` stay as they are.

Recommended: (b). Your reason was that the value must reach "the assignment operation". At a function's end the call gets a value from every arm either way.

An `if` at a function's end already needs its `else` today. Under both options, your Q-45 example `x := (if (c == 1) (return 5) else (i32 2)) + (free a, i32 1)` is now refused, since it sits in an assignment. The worker rewrites that test with an `if` that stands on its own line.

**Answer:** 
b

## Metadata
- **Status:** answered, relayed 2026-10-02 19:59
- **Priority:** the one point left in #82's `if` rules; the worker builds everything else meanwhile and keeps today's behaviour at a function's end
- **Asked:** 2026-10-02 19:44
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type` (dev merged in at 55e4b95), [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** the #82 worker (QUESTION #82, 2 October 19:43), through Orchestrator (2)
- **Waiting:** #82's worker, for the function-end case only
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-63_issue-82.md](../answers/Q-63_issue-82.md): your answers 3 and 4
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`if` reads its own right side‹, whose two Q-63 lines this narrows or keeps
