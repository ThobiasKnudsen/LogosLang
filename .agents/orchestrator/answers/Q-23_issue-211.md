# Q-23: `free (a[5])` over plain numbers: does the code that finds the element still run?

## Metadata
- **Status:** answered, relayed 2026-09-29 23:31
- **Priority:** 1 (why: #211's reviewer is idle until you answer; one letter answers it)
- **Asked:** 2026-09-29 23:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#211](https://github.com/ThobiasKnudsen/LogosLang/issues/211), one check for "is this a place" shared by `free`, `move`, `&` and `=`; its first slice is in review
- **Branch and worktree:** `issue-211-place-check`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-211)
- **Asked by:** logoslang-issue-211-review-1 (session b3c7f067), Opus 5.5
- **Waiting:** the reviewer is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your #210 answer](../answers/Q-16_issue-210.md): "there are many sets of code you can write which is unecessary so this shouldnt actually be an error"

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. Does `free` of an element run the code that reaches it?

`a[k]` is a call to the array's `at`, which ends in a dereference, so DESIGN counts it as a place. When the element's type has a `free` (a box), `free (a[k])` runs `at` (bounds check included) and then the box's `free`. When it has none (`i32`), the seed builds an "inert free" that runs nothing, so `at` never runs. One spelling behaves two ways:

```logos
a := array i32 [1, 2], free (a[5]), print «after»          # after   (index 5 never checked)
arr := array boxed [move x], free (arr[5]), print «after»  # run error: index out of range
get := fn (q := @i32 ?) -> i32 ( print «ran», q@ ), x := i32 3, free (get(&x)), print «after»   # after   (no "ran")
```

DESIGN says the inert free "runs nothing", but that was written for a plain name, where there is nothing to run. It does not say whether reaching a place through a call counts.

- (a) **Keep today's:** the inert free runs nothing, not even the code that finds the element. `free (a[5])` over `i32` is silent; over a box it is the index error.
- (b) **`free` always reaches its place:** the code that finds the element runs, as `a[5] = 3` runs it, bounds check included; only the teardown is skipped when the type has no `free`. `free (a[5])` is the index error for every element type, and `free (get(&x))` prints `ran`.

Recommended: (b), because one spelling then behaves one way whatever the element type, it matches `=` on the same call, and it matches your #210 reason: code you write runs.

**Answer 1:** 
trying to access index outof bounds should be an error anyways and i think its simpler to just get that at anyways even though the free actually doesnt free anything because the type of array could be dyad which means the type could be anything. so the access at the index should happen anyways. so option b
