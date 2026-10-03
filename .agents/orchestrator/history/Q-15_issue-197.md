# Q-15: a `fn` and the same function written as a type now build their body at different moments; which rule separates them?

## Metadata
- **Status:** answered, relayed 2026-09-29 22:21
- **Priority:** 1 (why: #197's review is idle until you answer; one letter answers it, and #197 then merges)
- **Asked:** 2026-09-29 22:05
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197)
- **Branch and worktree:** `issue-197-immediate-moment`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-2 (session 70161615), Opus 5.5
- **Waiting:** the reviewer is idle until this is answered; everything else on #197 is checked and green
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your first #197 answer](../history/Q-8_issue-197.md): "case 1 and 2 is correct since immediate executes right when its parsed"
  - [the other open question](../history/Q-14_issue-199.md): the seven `dyad` points, unrelated

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. The same function, two moments

DESIGN says a `fn` is shorthand for a type with a `run` body ("`fn` is shorthand for what `^` spells out", "exactly one specialization, built at definition"). Your #197 answers kept both behaviours: a `fn` body runs its `immediate` where it is written (case 1), and a type's `run` body waits for the first node (case 2). So the same function now runs `immediate` at two moments, depending on how it is spelled:

```logos
f := fn (a := i32 ?) -> i32 ( immediate ( print «hi», 1 ), a * a ), print «defined», f(3)
# hi defined 9

sq := type ( a := i32 ?, output_type := type ?, share run = ( immediate ( print «hi», 1 ), a * a ), … ), print «defined», x := i32 3, x sq
# defined hi 9
```

- (a) **The word decides.** `fn` builds its one body where it is written; a type written out with a `run` slot builds it at its first node. Both your answers stay word for word; the `fn` rule gets one clause saying `fn` is "the type, plus an early build".
- (b) **Both wait.** Once `fn` is a type (#127), its body is built at its first call too, so `f` above prints `defined hi 9`. This overturns case 1.
- (c) **Both are early when every field is typed and the output is declared.** `sq` above would print `hi` where it is defined; your `sq2`, which declares no output, still waits. This narrows case 2.

Recommended by the reviewer: (a), because it keeps both your answers as given, and the `fn` rule itself says "nothing a function does today changes".

**Answer 1:** 
the thing is that the body of fn cannot always be build at the definition site so it has to be parsed where its called later, but when it actually can be built at definition it should be built. that should likely be the case for type as well i think. btw would it be possible to partially parse some code section and stop and store its state for later when more information is known? that would be really nice and also alligns with the rule that says things should be lazy parsed when it can actually be parsed. that should probably be true for run as well but since it rarely gets everything it needs when the type is parsed its usually just stored as lexed until an instance of the type occurs with all the infromation it needs 