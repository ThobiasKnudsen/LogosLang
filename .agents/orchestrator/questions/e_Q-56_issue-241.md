# Q-56: May a name whose type has a `free` start empty, as `mut a := bag ?`?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. May `mut a := bag ?` start empty?

```logos
bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ),
mut a := bag ?,
a = bag(1)
```

Two of your answers point different ways:
- 29 September, #178 Q3: "Yes, bag ? owns too, in this issue". So `a` starts empty and owns what is written into it later.
- 1 October, Q-39, you chose (c): "Refuse an owning hole on a local name only, and keep it for a field and a parameter. A local owner then always gets its value at its declaration (`a := alloc 1 of i32 5` already makes `a` the owner), so it never starts empty." That question showed only `own` written over `?` (`mut a := own @i32 ?`).

Today's dev does neither: `a = bag(1)` is refused ("this is not an assignable place", problem P12), and when `a` is never written, its `free` runs anyway at the end and prints `freed 0` (problem P42).

- (a) **Allowed.** Q-39 refuses only the word `own` over `?`. `mut a := bag ?` starts empty and owns what is written into it; never written, nothing is freed. Cost: a write only inside an `if` arm, `if c ( a = bag(1) )`, leaves the scope's end unable to tell whether there is something to free, the same gap as #240.
- (b) **Refused.** A local name whose type has a `free` gets its value where it is declared: `mut a := bag(1)`. Your "bag ? owns too" becomes history. A field and a parameter keep `?`, as Q-39 says.

Recommended: (b). It is the "never starts empty" you chose in Q-39, and it removes P42 and the `if` arm case without the hidden flag ›`free` and `move` end a name; no drop flag‹ refuses. The cost is writing the value at the declaration.

**Answer 1:** 

## Metadata
- **Status:** open
- **Priority:** two of your rulings disagree; #241 and problem P42 wait for it; one letter
- **Asked:** 2026-10-01 21:43
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#241](https://github.com/ThobiasKnudsen/LogosLang/issues/241), `own` over `?` refused on a local name (your Q-39 "c")
- **Branch and worktree:** none yet: #241 waits for #215
- **Asked by:** Orchestrator (2), while checking #178's programs on dev f4d1fa3
- **Waiting:** #241's worker, before it starts; the fix of problem P42
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [2609300207_logoslang.md](file:///home/o/.claude/orchestrate/history/2609300207_logoslang.md): Q-39, your answer "c"
  - [#178](https://github.com/ThobiasKnudsen/LogosLang/issues/178), its Q3 of 29 September
