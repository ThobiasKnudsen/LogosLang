# Q-39: `own` on a hole: DESIGN allows `mut a := own @i32 ?` in four places. Which do you mean?

## Metadata
- **Status:** answered 2026-10-01 02:40, recorded, not relayed (its asker had ended): "c" is solution S19 for W76 (problem P37) in PWS.json and issue [#241](https://github.com/ThobiasKnudsen/LogosLang/issues/241); the word ruling ("hole" becomes "undefined") is on [#205](https://github.com/ThobiasKnudsen/LogosLang/issues/205#issuecomment-5922363034)
- **Priority:** 2 (why: nothing waits for it now; #192 goes to review without it, since the answer changes none of #192's code; one letter)
- **Asked:** 2026-09-30 02:22
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#192](https://github.com/ThobiasKnudsen/LogosLang/issues/192), the follow-up to Q-35 point 3. If your answer changes DESIGN, it gets an issue of its own.
- **Branch and worktree:** `issue-192-teardown-at-life-end`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-192)
- **Asked by:** logoslang-issue-192-worker (session c727a831), Opus 5.5
- **Waiting:** nothing; the rule on `own`
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-35](../history/Q-35_issue-192.md): your answer to point 3
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`move` is the act, `own` the gate word, `free` the end‹, ›Gate spelling: words left of `:=`; ownership in the reference type‹, ›A field may be `own @T ?`‹, ›A field may be `own t ?`, `t` a type whose body fills `share free`‹

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

## 1. What does `own` need a name for?

Your answer to Q-35 point 3: "mut a := own @i32 ?, shouldnt be allowed by the parser because own requires a name."

DESIGN allows that line in four places, and the seed accepts it (since the `own` rename of 29 September, not because of #192):
- ›`move` is the act, `own` the gate word, `free` the end‹ (your "yes own can stay as the gate name", 28 September): "`own` stands only in a type position and names a state: `mut items := own t ?`, `fn (p := own @i32 ?)` and `-> own @T` say that the field, parameter or result owns what is put into it."
- ›Gate spelling…‹: "`a := own @T ?` consumes (callee owns, source empties), `a := @T ?` borrows".
- ›A field may be `own @T ?`‹, its Seed line: "on a name too, `mut a := own @i32 ?` frees what is written into it."
- ›A field may be `own t ?`…‹: "A name works alike: `mut a := own t ?` owns what is written into it".

What it does today:

```logos
mut a := own @i32 ?,
a = alloc 1 of i32 5,
a@                # 5, and the block is freed when the scope ends
```

- (a) **Keep DESIGN:** `own` on a hole stays legal on a local name, as on a field or a parameter. "own requires a name" fits the act `move` (`b := move a` needs the name `a`), which was spelled `own` before 28 September.
- (b) **`own` becomes a gate word on the name**, left of `:=` like `mut`: `mut own a := @i32 ?`, `mut own items := t ?`, `fn (own p := @i32 ?)`. The type position goes; this respells the four places, array.logos (`mut ptr := own @element_type ?`) and `own`'s constructor.
- (c) **Refuse an owning hole on a local name only**, and keep it for a field and a parameter. A local owner then always gets its value at its declaration (`a := alloc 1 of i32 5` already makes `a` the owner), so it never starts empty.
- (d) **Something else:** say what `own` needs a name for.

Recommended: (a), if you read the line as the old act `own`: DESIGN and the seed agree, and nothing changes. If you meant that a local owner must never start empty, (c) is the smallest rule that says so.

**Answer 1:** 
c. btw i see there is another name i havent ruled which is hole. it should be undefined instead