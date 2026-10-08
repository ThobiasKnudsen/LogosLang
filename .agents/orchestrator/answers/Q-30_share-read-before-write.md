# Q-30: a `share` member declared with `?` in a type body: allowed, and when can it be read?

## Metadata
- **Status:** answered, relayed 2026-09-30 01:36
- **Priority:** 2 (why: the analyst is idle until this is answered, and its filing waits for it; one letter each)
- **Asked:** 2026-09-30 01:20
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `share-read-before-write` files after your answer. Found by the analysis of [#227](https://github.com/ThobiasKnudsen/LogosLang/issues/227) (`mut x := ?` makes `x` a second name for `?`)
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-share-read-before-write)
- **Asked by:** logoslang-rca-share-read-before-write-worker (session 26861891), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Reading a place before its first write is refused at parse‹, ›Slot words are known only inside a type body‹, ›Two muts, and the storage partition‹

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. `share mut s := i32 ?` in a type body: when can `pt.s` be read?

The seed never checks "read before first write" for a member read with `.`, so `pt.s` reads a zeroed place. Refusing the read before any write needs no ruling; what comes after the fix does. DESIGN lets a read through once a "sibling write" stands before it in the same block, but a type body cannot hold one: `s = 3` in a type body is an error. So by the letter, nothing can ever make `pt.s` readable:

```logos
pt := type ( a := i32 ?, share mut s := i32 ? ),
pt.s,            # 0 today; refused by DESIGN, since nothing is written yet
pt.s = i32 5,
pt.s             # 5 today; by the letter of DESIGN refused, forever
```

- (a) **A `share` line in a type body must have a value:** `share mut s := i32 ?` is an error there; write `share mut s := i32 0`. A `share` value is made once, at the definition (your answer of today: its initializer runs where it is written), so a `share` place never holds "nothing yet", and the read check never applies to it.
- (b) **A write through the type fills it:** `pt.s = i32 5`, as its own line (not inside an `if`, a loop or a `fn`), lets the reads after it in the same block through, so `pt.s` gives `5`. This would also settle the same open point on #227 (a `?` name written from outside its own block).
- (c) **DESIGN as it reads today:** the line is allowed, only its type can be read (`pt.s.type`), and `pt.s = i32 5, pt.s` is refused forever.

Recommended: (a), because it follows from "made once, at the definition" with no special case, and nothing is lost: you write the starting value.

**Answer 1:** 
actually i would like this to be the same as nonshare fields because it can be statically checked in the scope stack. so you cannot read it until it is written once first. but it differs in the sense that when it is written to once across the whole program it stays as written and if you add +1 every time some function runs then it increases by 1 each time so it grows towards the limit of that type then loops back to lowest value when highest value is met. does this make some things hard? i bet it conflicts with some things in the DESIGN.md. so option b

## 2. Only if (a): does a `share` line in a function body need a value too?

```logos
f := fn () -> i32 ( share mut k := i32 ?, k = i32 5, k )
```

This works today: the write stands before the read. But `k`'s place is made once, at the definition, and holds nothing until the first call writes it.

- (a) **Yes, the same rule everywhere:** write `share mut k := i32 0`.
- (b) **No:** in a function body, `?` stays allowed.

Recommended: (a), for one rule for every `share` line.

**Answer 2:** 
i chose option b for answer 1
