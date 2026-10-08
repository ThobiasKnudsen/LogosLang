# Q-52: A why run can climb past its problem: should each root be tested against the problem before its solution talk?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. Should a why run end with your root test?

P29 is a compiled function that runs on after a call fails:

```logos
p := alloc 1 of i32 5
g := fn () -> i32 ( error «stop» )
f := fn () -> i32 ( g(), p@ = 9, 1 )
f.compile()
f()
p@      # compiled: 9. Interpreted: 5, as it should be
```

Its why run climbed ten steps to "a scope's lines are run by three runners" (W71), which has three causes: W4 (every identity written twice, once per tier), W12, and W75. Each step passed the run's test, "does this, by itself, make the step below happen?". But nothing asked your root test, "if it were fixed, would the problem become impossible?". W75 fails it: its own solution agent says fixing it alone leaves P29 (Q-48), because P29 lives in the compiled tier, W4's branch. So W75 is a true structural fact, but not P29's root in your sense, and its solution talk now competes for your time with roots that do remove problems.

- (a) **End each run with the root test** for every root and its problem: "fixed alone, would the problem become impossible?" A root that fails stays (it is true), but its link to that problem is marked "contributes", not "removes", and solution talks for roots that remove problems come first. Cost: one step per run, one field in PWS.json.
- (b) **Ask the root test at every step**, not only the step below: a why whose fix would not help remove the problem is not taken. Shorter trees, but wide shared facts like "three runners", which the method is good at finding, may be missed.
- (c) **Leave it**: the solution agent says it, as W75's did.

Recommended: (a). It keeps the shared causes the run is good at finding and tells you which root's fix removes which problem.

**Answer 1:** 

## Metadata
- **Status:** open
- **Priority:** nothing waits; the why-run queue is held anyway while 15 roots wait for a solution talk; one letter
- **Asked:** 2026-10-01 03:20
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none, a question about how the why run works
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2), Opus 5.5
- **Waiting:** nothing
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [SKILL.md](file:///home/o/.claude/skills/orchestrate/SKILL.md): ›The why run‹, step 4, the test each why passes
  - [Q-48_W75.md](../answers/Q-48_W75.md): Q-48, where W75's solution agent says its fix alone leaves P29
