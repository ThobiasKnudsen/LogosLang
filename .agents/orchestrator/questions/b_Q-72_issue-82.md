# Q-72: #82 review: four points before the merge

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

#82's review rounds fixed the problems they found on the branch. Four points are left that only you can rule on, and the merge waits for them. Each point stands alone.

## 1. May an `if` or a block keep its type in the node?

Two DESIGN rules disagree:
- ›`if` reads its own right side‹, your Q-59: "None of the three has an `output_type` field, as `return` has none" ("they have no output_type field like return").
- ›A node's output type is per node, and its parse writes it‹: "the parse writes `tape[0].output_type = tape[-1].type`; use sites and the lowering read it from the node".

The branch follows the second. An `if` node keeps its arms' type, and a block keeps its last line's type, so this works:

```logos
mut c := true,
b := if c (i32 1) else (i32 2),
b:start.rhs.output_type          # i32, stored in the if node
```

- (a) **Keep the stored type** as a temporary stand-in.
- (b) **Store nothing.** The type is worked out when asked: an `if`'s from its arms, a block's from its last line.

Recommended: (b). It is what you said in Q-59. A stored copy must be updated after every rewrite of an arm or a last line, and if one is missed nobody notices.

**Answer 1:** 

## 2. One arm makes a value, the other borrows one: who owns it?

```logos
c := true,
a := box (1, 2),
x := if c (box (3, 4, 5)) else (a)
```

When `c` is true, `x` should own the new box and free it. When `c` is false, `x` only borrows `a`, which `a` frees. Which arm runs is known only when the program runs. But ›`move` and `free` are static‹ says who owns a value is always known before it runs: "There is no "maybe moved"".

- (a) **Refused where it is written:** "make a value in every arm, or borrow in every arm". `free (if c (box (3, 4, 5)) else (a))` is already refused for the same reason.
- (b) **The borrowing arm makes a copy,** so every arm makes a value. This needs the copy-or-reference rule, which is not ruled yet.
- (c) **Decide when it runs** with a hidden "made or borrowed" flag. The static rule above rules this out.

Recommended: (a). It refuses rather than guesses, it matches `free`, and the owner stays known before the program runs.

Round 3 (90bd6f8) brought back dev's refusals for the same mix in a call argument and a `return`, `h(if c (alloc …) else (move b))` and `return if c (move b) else (alloc …)`, through a marked stand-in. Under (a) that stand-in is deleted and one refusal covers all three places.

**Answer 2:** 

## 3. What does `return` do outside every function?

```logos
x := i32 4,
return x        # dev: prints 4.  The branch: prints nothing.
```

›A scope's value is what it evaluates to, and `return` is…‹ says "`return X` is written only to leave a function early". Since your Q-59, a `return` gives its own line nothing. At the top level there is no function to leave.

- (a) **It gives nothing,** as on the branch now. The line just runs and nothing is printed.
- (b) **Refused:** "`return` is written only inside a function".
- (c) **It ends the program,** and its value is printed as the program's value. `return 5, 6` would print `5`. Today it prints `6`.

Recommended: (b). A `return` that does nothing hides a mistake, and DESIGN says a `return` exists only to leave a function. This also settles problem P55 (#234): today `return 5, 6` prints `6`.

**Answer 3:** 

## 4. Does `free (…)` use the value it frees?

Added at 22:20 from review round 3. Your Q-63 rules say "an `if` whose value is used must have an `else`" and every arm must give a value. No rule says whether `free`'s operand counts as such a use:

```logos
mut c := true,
free (if c (alloc 1 of i32 5)),                    # no else: runs, frees nothing, the block leaks
free (if c (alloc 1 of i32 5) else (y := i32 2)),  # one arm gives nothing: refused today
free (x := i32 5)                                  # a statement: runs, frees nothing
```

- (a) **Yes, `free` uses its operand.** All three lines are refused with the reason any use gets ("needs an `else`", "this arm gives nothing", "this gives nothing").
- (b) **No.** `free` of something that gives nothing runs it and frees nothing. The leaked block in line 1 is then freed at its line's end, once #224 is built.
- (c) **Keep today's mix:** lines 1 and 3 run and free nothing, line 2 is refused.

Recommended: (a). `free` exists to end a value; when nothing is given there is nothing to end, and line 1 leaks without a word today.

**Answer 4:** 

## Metadata
- **Status:** open
- **Priority:** the merge of #82 waits for these four points, and eight issues wait for that merge, so it is first
- **Asked:** 2026-10-02 21:06
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type` at 90bd6f8, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** logoslang-issue-82-review-1 (points 1-3, round 1, 2 October 21:05) and logoslang-issue-82-review-3 (point 4, round 3, 22:20), through Orchestrator (2)
- **Waiting:** the merge of #82; round 3's reviewer waits for these answers
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-59_issue-82.md](../history/Q-59_issue-82.md): your words on `if` having no `output_type` (point 1)
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): the rules quoted above
  - Review record on #82: [comment 5959473810](https://github.com/ThobiasKnudsen/LogosLang/issues/82#issuecomment-5959473810)
