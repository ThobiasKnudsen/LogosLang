# Q-24: inside a function, what does `a:start` of a parameter show?

## Metadata
- **Status:** answered, relayed 2026-09-29 23:44
- **Priority:** 1 (why: #199's reviewer is paused until you answer; one letter answers it)
- **Asked:** 2026-09-29 23:46
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), a bare parameter `fn (a)` is a `dyad ?` holding a node of any type; the review of your recorded answers runs now
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-review-1 (session a0851c85), Opus 5.5
- **Waiting:** the reviewer is paused until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your answer that `a:start` shows the graph](../history/Q-18_issue-199.md): "if you do a:start you get to the actuall LG where a is declared and defined"
  - [the other #199 points](../history/Q-22_issue-199.md): point 2 there was rewritten to ask both readings

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. `a:start` of a parameter, when the function is called twice

```logos
f := fn (a) -> i64 ( a ),   # imagine the body also reads a:start
x := i64 5,
y := i64 9,
f(x + 1),   # a:start should show  a := x + 1
f(y * 2),   # a:start should show  a := y * 2
```

Two of your rulings disagree here:
- Your #199 answer today: "if you do a:start you get to the actuall LG where a is declared and defined where you can inspect the nodes a, :=, x, + and 1". That gives each call its own `a:start`.
- Your ruling of 28 September, ›A binding's fields are read at elaboration, at the line of the read; running code never reads a binding‹ ("never runtime"): "`a:start` … read when the node holding the read is built … A body that runs later, a `fn` body at a call …, gives the same answer."

Also, a function body is built once per set of argument types. Both calls pass an `i64`, so they share one build, and `a:start` is fixed in that build: it can show `a := x + 1` or `a := y * 2`, not both.

- (a) **`a:start` of a parameter is where `a` is written, `fn (a)`,** the same for every call. The argument's graph still exists, but you reach it from the call (`f(x + 1)`'s operand), not through `a`.
- (b) **A body that reads a parameter's `:start` is built once per call site** instead of once per set of types, so each call site has its own `a := x + 1`. Bodies that do not read it keep sharing one build per set of types.
- (c) **`a:start` of a parameter is read when the call runs:** an exception to "never runtime".

Recommended: (b), because it keeps both of your rulings, and the only cost is an extra build, only for bodies that inspect their parameter's graph. (a) is the simplest, but it drops what you said `a:start` shows. (c) reopens "never runtime", whose reason was that "a binding read at run would be a lookup the graph should already have settled".

**Answer 1:** 
option a
