# Q-65: an `if` whose arms have one type, where no type is required: write the type or not?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Your Q-63 answers are in DESIGN on dev as a632385. Your answer 2 refuses `x := if c (i32 1) else (i64 2)` as ambiguous and has it written `x := i64 if c (i32 1) else (i64 2)`. Your words can be read two ways for arms that have one type:

```logos
x := if c (1) else (2),            # both arms rational_number
y := if c (i32 1) else (i32 2),    # both arms i32
```

- (a) **Only arms whose types differ need the type written.** Arms of one type give that type: `x` is a `rational_number` and `y` an `i32`, as today. A plain number beside an `i32` arm is a different type, so `z := if c (i32 1) else (0)` is written `z := i32 if c (i32 1) else (0)`.
- (b) **Every `if` that gives a value where no type is required has its type written in front:** `y := i32 if c (i32 1) else (i32 2)`. DESIGN's example `x := if c (d) else (e)`, where `d` and `e` are both `dyad ?`, is then written `x := dyad if c (d) else (e)`.

Recommended: (a). Your reason was "it is ambiguous what type x should be", and arms of one type leave nothing ambiguous. It also keeps today's programs as they are.

**Answer:** 
a

## Metadata
- **Status:** answered, relayed 2026-10-02 19:35
- **Priority:** the last open point of #82's `if` rules; until then the worker keeps today's behaviour (arms of one type give that type), so it sits after Q-64, which decides when #82 can merge
- **Asked:** 2026-10-02 19:19
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** Orchestrator (2), from your Q-63 answer 2 (Q/A on #82, comment 5957585903)
- **Waiting:** #82's worker, paused until 4 October 9:00 by the weekly usage limit
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-63_issue-82.md](../history/Q-63_issue-82.md): your answers this builds on
  - [03_Q-64_issue-82.md](../history/Q-64_issue-82.md): the other open #82 question
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`if` reads its own right side‹, whose Open (Q-65) line this is
