# Q-16: what do `free`, `move` and `&` do with something that is not a place?

## Metadata
- **Status:** answered, relayed 2026-09-29 22:29
- **Priority:** 3 (why: nobody is idle on it; #211's worker builds its first slice without it, and its third slice and #117 wait for it; two letters answer it)
- **Asked:** 2026-09-29 22:19
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#210](https://github.com/ThobiasKnudsen/LogosLang/issues/210); it settles slice 3 of the root [#211](https://github.com/ThobiasKnudsen/LogosLang/issues/211), and [#117](https://github.com/ThobiasKnudsen/LogosLang/issues/117)
- **Branch and worktree:** the #211 worker is on `issue-211-place-check`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-211)
- **Asked by:** filed as #210 by logoslang-rca-free-expression-operand-review-4 (session fdfacd38, ended), Opus 5.5
- **Waiting:** the #211 worker works on; until you answer, it refuses these operands as a stand-in
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the #197 question](../history/Q-15_issue-197.md): unrelated; answer it first, one letter unblocks a merge
  - [the analysis](file:///home/o/.claude/orchestrate/rca/logoslang/free-expression-operand.md): every probe with its output

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

DESIGN says what a place is: a name, a field path, a dereference, a cell, and a call that ends in a dereference. It says what `free`, `move` and `&` do with a place, and nothing about anything else. The seed answers four different ways today (`f` as defined in the first line):

```logos
f := fn () -> i32 ( print «ran», 7 ), free (f()), 1   # 1, and f never runs
free (alloc 1 of i32 5), 1                            # 1, and the alloc never runs
move (f()), 1                                         # error: this is not an assignable place
p := &(f()), 1                                        # error: `&` needs a variable to take the address of
```

## 1. `free` or `move` on something that is not a place

```logos
free (f())
free (alloc 1 of i32 5)
move (f())
```

- (a) **A checked error.** The message says what to write instead: `f()` alone runs it, `v := f(), free v` names it first. Nothing is lost: running a value and throwing it away is already `f()` on its own line.
- (b) **Run it, then free it.** `free (f())` prints `ran`, and `free (alloc 1 of i32 5)` allocates and frees at once, like Rust's `drop(f())`. DESIGN then needs a sentence making `free` something that holds a value, an exception to "an owning value must be bound to a name".

Recommended: (a), because every DESIGN example of `free` and `move` is a place, and it needs no exception to that rule.

**Answer 1:** 
i think the free examples should work but they are unecessary but there are many sets of code you can write which is unecessary so this shouldnt actually be an error but the move line should be because you cannot move something anon. 

## 2. `&` of a value that nothing names

```logos
w := type ( x := i64 ? ), p := &w(7), p@.x          # today: error
p := &(1 + 2), p@                                    # today: error
w := type ( x := i64 ? ), q := w(7), p := &q, p@.x  # 7, works today
```

#117 and #99 (12 September) read DESIGN as "yes" for a record (`&w(7)`) and "no" for `&(1 + 2)`. DESIGN draws no line between the two: both are values nothing names.

- (a) **A checked error, as in 1 (a).** The message says to name it first: `q := w(7), p := &q`. #117 then closes.
- (b) **`&` keeps the value alive until its scope ends and hands out its address,** as Rust's `let p = &w(7);` does. DESIGN then needs a sentence saying `&` holds it, where its life ends, and that its `free` runs there.

Recommended: (a), because then all four place words (`=`, `&`, `move`, `free`) follow one rule: a place, or a checked error. If you prefer (b), it belongs with 1 (b), as one rule: "a word that takes a value holds it".

**Answer 2:** 
b
