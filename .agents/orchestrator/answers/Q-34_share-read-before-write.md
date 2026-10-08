# Q-34: your `share` rule (read only after the first write): what it makes hard, what DESIGN changes, and two choices

## Metadata
- **Status:** answered, relayed 2026-09-30 01:51
- **Priority:** 2 (why: the analyst is idle and its filing waits; the first part answers your question, so it needs reading; then one letter each)
- **Asked:** 2026-09-30 01:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `share-read-before-write` files after your answer
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-share-read-before-write)
- **Asked by:** logoslang-rca-share-read-before-write-worker (session 26861891), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-30](../answers/Q-30_share-read-before-write.md): your answer, "the same as nonshare fields … does this make some things hard? i bet it conflicts with some things in the DESIGN.md". Its point 2 asked only in case of (a), so it fell away.
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Two muts, and the storage partition‹, ›Reading a place before its first write is refused at parse‹, ›A value of several fields is filled one field at a time‹, ›Integer `/` and `%` are total and saturate; float `%` does not exist‹
  - [RCA.md](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-share-read-before-write/RCA.md): the analysis and the probes behind each line

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## Your question: does it make things hard, and does it conflict with DESIGN?

Your rule works, and it is cheap to check. It is the rule a field of `v := pt ?` follows today, with the type's name in `v`'s place. The parse reads the text once, in order, so "written once" means "written by an earlier line that is not inside an `if`, a loop or a `fn` body". After that the place keeps its value, and a `+1` counter works as today.

```logos
pt := type ( a := i32 ?, share mut s := i32 ? ),
pt.s,                                   # refused: nothing written yet
pt.s = i32 5,
pt.s,                                   # 5
get := fn () -> i32 ( pt.s ), get()     # 5: `get` is written after the write
```

**What it makes hard:** little in the seed, since the member already carries a "not written yet" mark. One real limit: the type's own functions can never read a member declared with `?`, because they are written inside the type body, before any write can stand.

```logos
pt := type ( a := i32 ?, share mut count := i32 ?, share next := fn () -> i32 ( count = count + 1, count ) )
# refused at `count`. With `share mut count := i32 0` it works: p.next(), p.next() gives 2
```

And, as for names today, a write inside a function or an `if` does not count:

```logos
mut x := i32 ?, set := fn () -> void ( x = i32 5 ), set(), x    # refused
```

**What DESIGN changes:** two sentences.
- ›Two muts…‹: "Its value is made once, at the definition." gains "or, declared with `?`, at its first write".
- ›Reading a place before its first write…‹: "a sibling write in text order fills the name from that line". A type's member has no sibling line, since a type body cannot hold `s = 3`. The rule needs one sentence on which write fills a member: question 1.

It agrees with ›A value of several fields is filled one field at a time‹ (your rule is that one) and with ›Filling a valueless declaration needs `mut`‹ (a `share s := i32 ?` without `mut` can never be filled).

COMMENT: im quite sure that set() should be made possible in logos thoug. is that possible?

## 1. Which write fills a type's member?

- (A) **A line of the block that declares the type** (the block holding `pt := type (…)`), as for a field of `v := pt ?`. A file that imports `pt` counts as such a block for its own lines. So `( pt.s = i32 5, pt.s )` is refused, as `mut x := i32 ?, ( x = i32 5, x )` is today.
- (B) **A line of any block**, for the reads after it in that block and the blocks inside it: `( pt.s = i32 5, pt.s ), pt.s` passes inside and is refused outside. Names get the same, so `mut x := i32 ?, ( x = i32 5, x )` passes too: a bigger change.

Recommended: (A). It is the field rule unchanged, one mark per name as now.

**Answer 1:** 
first ive added a question to the answer from you above. why shouldnt B work though? i would like that to work. its the same as mutating a variable in outside scope inside an if statement. does this conflict with any borrowchecking and lifetimes though?

## 2. Integers at their limit: do `+`, `-`, `*` wrap?

You wrote that the counter "loops back to lowest value when highest value is met". DESIGN has no rule for `+`, `-`, `*` past an integer's limit. The seed wraps, on both tiers:

```logos
x := i32 2147483647, x + 1       # -2147483648
mut n := u8 255, n = n + 1, n    # 0
```

But `/` and `%` saturate (›Integer `/` and `%`…‹: "`x/0` gives MAX for a dividend ≥ 0"), and a rational out of range is an error, "not a wrap".

- (A) **Wrap:** a new rule. `/` and `%` keep saturating, and the rule says the two behaviours differ on purpose.
- (B) **Saturate:** `+`, `-`, `*` stop at the limit like `/`, one behaviour.
- (C) **An error** at the limit.

Recommended: (A). It is what you described and what runs now.

**Answer 2:** 
those run on instructions on hardware so they should wrap though. option a