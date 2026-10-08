# Q-37: `set()` can count as a write, and (B) works: which write fills a place first?

## Metadata
- **Status:** withdrawn unanswered 2026-10-01 01:48, not needed now (Thobias asked to remove the questions not needed): it was asked so the analysis could file, and PWS replaces that filing. The finding is problem P34 in PWS.json; this point comes back in the solution talk for P34's root.
- **Priority:** 1 (why: the analyst is idle and its filing waits only for this; one letter, and it answers your two questions)
- **Asked:** 2026-09-30 02:05
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `share-read-before-write` files after your answer
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-share-read-before-write)
- **Asked by:** logoslang-rca-share-read-before-write-worker (session 26861891), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-34](../answers/Q-34_share-read-before-write.md): your questions "is that possible?" (`set()`) and "why shouldnt B work though? … does this conflict with any borrowchecking and lifetimes though?"; your answer 2 (wrap) is recorded
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Reading a place before its first write is refused at parse‹, ›A call is a use of every outer name the callee's body reads‹, ›`move` and `free` are static: the parse marks the name dead‹, ›Base stratum: types, ownership, capabilities, and one borrow rule‹

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## Your two questions

**Can `set()` count as a write of `x`?** Yes. It mirrors a rule DESIGN already has: a call is a use of every outer name its body reads. The mirror: a call standing as its own line writes every outer name its body writes as its own line.

```logos
mut x := i32 ?, set := fn () -> void ( x = i32 5 ), set(), x                                 # 5
mut x := i32 ?, set := fn () -> void ( x = i32 5 ), g := fn () -> void ( set() ), g(), x     # 5: calls count too
mut x := i32 ?, set := fn () -> void ( if c ( x = i32 5 ) ), set(), x                        # refused: it writes only if c
```

Nothing that works today breaks; more programs get through.

**Why might (B) not work, and does it meet borrow checking or lifetimes?** (B) works. It replaces one sentence of ›Reading a place before its first write…‹: "A write inside a nested block, loop or `fn` body does not" fill the name. A write then fills the place for the reads after it in its own block, and after the block closes, reads are refused again. The borrow rule ("many readers XOR one writer") is about overlapping references, not about when a read may happen, and (B) does not change when a name is live. So there is no conflict with either.

```logos
mut x := i32 ?, ( x = i32 5, x )                        # 5 (refused today)
mut x := i32 ?, ( x = i32 5 ), x                        # still refused: this read is outside the block
mut x := i32 ?, if c ( x = i32 5, print «{x}» ), x      # the print passes; the last `x` is refused: the arm may not run
```

## 1. Which write fills a place first?

- (A) **Only a line of the block that declares the name**, as recommended before.
- (B) **A line of any block**, for the reads after it in that block; and a call writes what its callee's body writes as its own lines, so `set()` works.
- (B+) **As (B), and a write in every arm of an `if` fills the name after the `if`:**
  ```logos
  mut x := i32 ?, if c ( x = i32 5 ) else ( x = i32 6 ), x    # passes
  ```
  This is how a `move` in an arm already works: after the `if`, the name is in the same state on every path.

Recommended: (B+). Each part mirrors a rule DESIGN already has (a call is a use; the `move` rule), so no new kind of rule is added. The cost: the replaced sentence goes to DESIGN_HISTORY.md, and the checker keeps a small mark per open block.

**Answer 1:** 
