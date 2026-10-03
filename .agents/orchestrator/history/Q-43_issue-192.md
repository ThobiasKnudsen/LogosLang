# Q-43: a `free` in a part of a line that doesn't run: refuse it, or free the value some other way?

## Metadata
- **Status:** answered, relayed 2026-10-01 01:12
- **Priority:** 0 (why: #192 is done except for this; its merge unblocks the next builders; one letter)
- **Asked:** 2026-09-30 03:07
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), a value is freed where its life ends
- **Branch and worktree:** `issue-192-teardown-at-life-end`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-192)
- **Asked by:** logoslang-issue-192-review-1 (session 45dc29f9), Opus 5.5
- **Waiting:** #192's review, then its merge; the next builders wait for that
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`move` and `free` are static: the parse marks the name dead‹, ›`free` and `move` end a name; no drop flag‹, ›A checked error is a fault: the task that hit it is cancelled‹, ›No implicit coercion; a numeric type applied to a value is the conversion‹ ("Logical operators short-circuit")
  - [#192's review record](https://github.com/ThobiasKnudsen/LogosLang/issues/192#issuecomment-5902039456): round 1 fixed 7 findings, 4 of them crashes
  - [#217](https://github.com/ThobiasKnudsen/LogosLang/issues/217), what an `if` or a scope hands back is worked out by each reader on its own; [#224](https://github.com/ThobiasKnudsen/LogosLang/issues/224), a line's end frees a value nothing took

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

When a line frees or moves a name, the parse marks the name dead for that whole line. If the part of the line that frees it doesn't run, nothing frees the value:

```logos
bag := type ( mut n := i32 ?, share free = ( print «freed {n}» ) ),
c := i32 0, a := bag(1),
x := (c == 1) and ( free a, c == 0 ),   # `and` skips its right side, so `free a` never runs
COMMENT: why would "and" skips its right side? it shouldnt

print «after {x}»
# prints: after 0      (never "freed 1": the value leaks)
```

Written with an `if`, it works, because an `if` frees in the arm that did not run:

```logos
x := if (c == 1) ( free a, c == 0 ) else ( false )
# prints: freed 1
```

The same leak happens when an earlier part of the line fails:

```logos
g := fn () -> i32 ( error «stop» ),
f := fn () -> i32 ( a := bag(1), x := g() + ( free a, i32 1 ), 1 ),
f()
# prints only: run error: stop      (never "freed 1")
```

The seed can't check at run time whether the freeing part ran, because DESIGN has no run-time flag for it (›`free` and `move` end a name; no drop flag‹).

## 1. What should happen?

- (a) **Refuse it.** A `free` or `move` of a name from outside the brackets must be the first thing its line runs, or sit inside an `if`. `x := (c == 1) and ( free a, c == 0 )` and `f(g(), move a)` become errors that say: write an `if`, or split the line (`y := g(), f(y, move a)`). `b := move a`, `consume(move a)` and `if (…) ( free a )` keep working. It is the same kind of rule as the one that already refuses a `move` of an outside name inside a loop.
- (b) **`and` and `or` free what their skipped side would have ended**, as an `if` does. A failure earlier in the line still leaks; today a failure ends the program, so the loss is only that value's `free` not running (a print, a file close).
- (c) **Every operator and call frees what a later part would have ended**, when an earlier part fails or is skipped. No errors and no leaks, but each of them carries a list of what its parts end.

The reviewer recommends (a): one sentence of rule, no new mechanism, and where a life ends stays exact everywhere without a flag. Your reason on #210 (29 September) leans the other way: "there are many sets of code you can write which is unecessary so this shouldnt actually be an error".

**Answer 1:** 
i dont think a runtime flag is needed because all things that could possibly be freed when error occurs is not NULL so it just checks if the values for each name is not NULL and if so it frees it.

**Follow-up (Orchestrator (2), 1 October 2026).** Your comment and your answer each go against a sentence DESIGN.md already has, so I can't pass them on as they stand. Two questions:

## 2. Should `and` and `or` always run both sides?

Your comment above: "why would "and" skips its right side? it shouldnt". DESIGN.md says they skip, in ›No implicit coercion; a numeric type applied to a value is the conversion‹: "Logical operators short-circuit." No reason is written next to that sentence. The seed skips today, and no `.logos` file in the repo counts on the skip.

- (a) **Both sides always run.** `and` and `or` become plain operators over two bools, and `if` is the only thing that runs one part and skips another. Question 1's first example then frees `a`, so that leak can't happen. The cost: code that counts on the skip must use an `if`. This line reads `xs[i]` even when `i` is past the end:
  ```logos
  ok := i < n and xs[i] > 0
  ```
  so it must be written:
  ```logos
  ok := if (i < n) ( xs[i] > 0 ) else ( false )
  ```
- (b) **Keep the skip**, as DESIGN says. Then question 1's first example still needs one of its options (a), (b) or (c).

I recommend (a): it deletes a special case instead of adding a fix for it. The cost is surprise: C, Rust, Python and JavaScript all skip.

**Answer 2:** 
a

## 3. Checking for NULL when a line fails, where DESIGN says compiled code checks nothing

Your answer: "it just checks if the values for each name is not NULL and if so it frees it."

DESIGN.md, ›`free` and `move` end a name; no drop flag‹: "`free` and `move` both read the value and end the name, leaving the source `undefined`. [...] So no teardown runs over an emptied place, there is no run-time drop flag, and compiled code checks nothing". Its reason: "with the tag bits gone the flag had no clean home, and a lifetime should be a fact the graph states, not a byte the run looks up."

For your check to work, `free` and `move` must leave NULL behind instead of `undefined`, and the teardown must look at each place when a line fails. That is a drop flag with NULL as the flag. It needs no extra byte, so it answers the first half of the rule's reason, not the second.

- (a) **Your NULL check, only when a line fails.** `free` and `move` write NULL into the place they end. When a line fails, the teardown frees every place of the scopes the failure stops that is not NULL. Lines that don't fail still check nothing. The rule gets a Ruled line saying so. Every value with a `free` slot then needs a NULL form: a pointer has one, but a value stored directly in its place needs one too.
- (b) **No check: the order of the line says it.** A line runs left to right, so each spot where it can fail knows which of its `free`s and `move`s have already run. The teardown there frees exactly the names still alive at that spot. In the third example, a failure inside `g()` frees `a`, because `free a` comes after `g()`. Nothing is written or checked at run time, and the rule stays as it is. This is question 1's (c), but only for failures.

I recommend (b): it keeps the rule, and no value needs a NULL form.

**Answer 3:**  
a