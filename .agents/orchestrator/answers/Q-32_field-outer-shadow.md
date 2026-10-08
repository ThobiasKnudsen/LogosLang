# Q-32: may a type body's slot words (`run`, `parse`, `parse_rank`, …) stand over an outer name of the same spelling?

## Metadata
- **Status:** withdrawn unanswered 2026-10-01 01:48, not needed now (Thobias asked to remove the questions not needed): it was asked so the analysis could draft its second slice, and PWS replaces that filing. The finding is problem P32 in PWS.json; this point comes back in the solution talk for P32's root.
- **Priority:** 3 (why: only the second slice of a root being filed waits for it; one letter)
- **Asked:** 2026-09-30 01:26
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `field-outer-shadow` files a new root (a field is checked against its siblings only, so `x := 1, p := type ( x := i32 ? )` is accepted) after this
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-field-outer-shadow)
- **Asked by:** logoslang-rca-field-outer-shadow-worker (session 1beee50f), Opus 5.5
- **Waiting:** the analyst waits for this before it drafts its second slice
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Name resolution is scope-filtered, and shadowing is disallowed‹, ›Slot words are known only inside a type body‹, ›Fields are read with a dot outside their scope‹
  - [#230](https://github.com/ThobiasKnudsen/LogosLang/issues/230): a body built later is checked against names declared after it (a different root)

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. Slot words over an outer name: allowed, or the shadowing error?

DESIGN says a field may not stand over an outer name: `x := 1, p := type ( x := i32 ? )` is the error (your 27 September ruling). The seed does not check that yet, and fixing it needs no ruling. But every type body also declares its slot words itself (`parse_rank`, `lex_rank`, `associativity`, `parse`, `run`), and those can stand over an outer name of the same spelling:

```logos
parse_rank := i32 5,
p := type ( a := i32 ?, share parse_rank = 60 ),   # the body's `parse_rank` stands over the outer one
print «{parse_rank}»                               # 5 today

a := type ( share b := type ( share parse_rank = 61 ), share parse_rank = 60 ),
print «{a.parse_rank} {a.b.parse_rank}»            # 60 61 today: a type body inside a type body
```

- (a) **Slot words are the one exception:** `type` declares them in every body, and you never write their declaration, so they may stand over an outer name, and the innermost wins. Everything you write yourself gets the no-shadowing check. Both programs keep working.
- (b) **No exception:** the first program is the shadowing error, and a type body inside a type body is refused, so the second fails.
- (c) **Slot words are not names at all:** `share run = …` reaches the place `type` declares the way `.` reaches a field, so no lookup ever finds two. A new mechanism.

Recommended: (a), because ›Slot words are known only inside a type body‹ already says so ("A lookup finds the innermost"), it is the smallest change, and nested type bodies keep working.

**Answer 1:** 
