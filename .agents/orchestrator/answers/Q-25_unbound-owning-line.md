# Q-25: a value nobody names, like `box (1, 2)` alone on a line: an error, or freed at the end of its line?

## Metadata
- **Status:** answered, relayed 2026-09-30 00:04
- **Priority:** 1 (why: an analysis is idle until you answer; one letter answers it, and it also settles point 2 of #183)
- **Asked:** 2026-09-29 23:53
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet; the analysis `unbound-owning-line` files its root once you answer. It also settles point 2 of [#183](https://github.com/ThobiasKnudsen/LogosLang/issues/183), which asks you to confirm the reasons an agent wrote for your #168 rulings (how a scope lays out its names)
- **Branch and worktree:** none; the analysis is in [its worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-unbound-owning-line)
- **Asked by:** logoslang-rca-unbound-owning-line-worker (session 7853e6b7), Opus 5.5
- **Waiting:** the analysis is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your #210 answer](../answers/Q-16_issue-210.md): "there are many sets of code you can write which is unecessary so this shouldnt actually be an error"

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. An error, or freed at the end of its line?

Today the seed neither refuses nor frees such a value. `<BOX>` is a test type whose `free` prints `free`:

```logos
<BOX>, box (1, 2), print «after»                        # after          (never freed)
<BOX>, a := box (1, 2), print «after»                   # after, free    (named: freed)
<BOX>, mk := fn () -> boxed ( box (1, 2) ), mk()        # never freed
import ./identities/array.logos, (array i32 [1, 2])[1]  # 2, and the array is never freed
alloc 1 of i32 5, 1                                     # 1, and the memory is never freed
```

DESIGN says it is an error, in two rules from July, when a value's `free` was still inserted where its name was declared: "A value that reaches no name at all is a checked error" (›Holding is decided at the binding site, parameters included‹) and "An owning value must be bound to a name" (›Three fail-closed ownership rules‹, rule 1). Against them: your 28 September ruling on #168 says a value nobody names is "freed with the line", and your #210 answer today allowed `free (alloc 1 of i32 5)` because unnecessary code should not be an error.

- (a) **An error.** `box (1, 2)` alone, `mk()` alone, `(array i32 [1, 2])[1]`, `print «{box (1, 2)}»` and typing `box (1, 2)` in the REPL are all refused. You write `a := box (1, 2)` or `free (mk())` instead.
- (b) **It lives to the end of its line, and its `free` runs there,** as Rust drops a temporary at the end of a statement. `box (1, 2), print «after»` prints `free`, `after`; `(array i32 [1, 2])[1]` gives `2` and frees the array; `mk()` alone frees what it returns. The two July error rules go.

Recommended: (b), because it is your #210 reason applied to the same kind of code, it is what your #168 ruling already says for the bytes, and reads like `mk().size` and the REPL echo keep working. The end of a line becomes one more place a life ends, beside a scope's exit, `=` and `free`.

**Answer 1:** 
i dont really understand what box actually does or what its purpose is so its hard for me to give a good answer here.  but if you are sure b is actually consistent with what ive said earlier and you are really sure then you can go for it