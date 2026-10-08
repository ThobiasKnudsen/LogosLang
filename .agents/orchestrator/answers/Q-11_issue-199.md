# Q-11: with bare `fn (a)` now `a := dyad ?`, is `fn (a) -> i64 ( a )` refused, or does it run?

## Metadata
- **Status:** answered, relayed 2026-09-29 21:07
- **Priority:** 1 (why: the #199 worker is idle until you answer; one letter answers it, and it decides which issue owns the seed change)
- **Asked:** 2026-09-29 21:05
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199); the compile question you asked for is filed as [#201](https://github.com/ThobiasKnudsen/LogosLang/issues/201)
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-worker (session 6dd47466), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your answer on #199](../answers/Q-9_issue-199.md): "a should be of tyoe dyad since that is the only way to have dynamic types but that also means this function cannot be compiled, or it can actually be compiled but it needs specific compilation for each callee since the type may vary"

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. What does this program do?

```logos
f := fn (a) -> i64 ( a ),
f(i64 42),
```

Your ruling is recorded in DESIGN.md: bare `a` is `a := dyad ?`. DESIGN says a `dyad ?` holds a node, never a number (`mut d := dyad ?, d = 5` is a parse error), and a node is not an `i64`. So by DESIGN, `f` is refused where it is defined. Your words about compiling "since the type may vary" sound instead like `f` works and `a` holds values of different types per call. The sketch also passes numbers to bare parameters: `f := fn (x, y) -> ? ( ? )`, then `f(x + d, y)`.

What the seed does today:

```logos
g := fn (a := dyad ?) -> i64 ( a ), g(i64 5)        # refused: "these types do not match"
g := fn (a := dyad ?) -> bool ( a == i32 ), g(i32)  # true
```

- (A) **Refused, as DESIGN says.** `a` holds a node, and the body asks about it at run: `a:type == i32`. A function that takes any number, checked per call, is written `fn (a := ?)`. Nothing in DESIGN changes; the sketch's lines become `fn (x := ?, y := ?)`. Type mistakes stay parse errors, and #198 owns the seed change.
- (B) **It runs, `f(i64 42)` gives `42`.** `a` holds the value together with its type; landing it in `-> i64` is checked when the body runs, so `f(f64 2.5)` is a run-time error. Two DESIGN rules change: a `dyad ?` may hold a number, and a typed place accepts a `dyad` checked at run. Each number passed this way costs one allocation per call. A new run-time check is needed that no issue covers yet.

Recommended by the worker: (A), because it is the option you picked as it was written, needs no new rule, and keeps every type mistake a parse error; dynamic types stay available through `a:type`.

**Answer 1:** 
i want it to actually work and the way i think it should work is to define dyad so that its transparent and a placeholder for a new node with any type so when you do a.something it doesnt access the value for dyad but rather the value the dyad.value is pointing to and if its empty/NULL you have to look at the type of what is tryig to be written and writes the type first then the value