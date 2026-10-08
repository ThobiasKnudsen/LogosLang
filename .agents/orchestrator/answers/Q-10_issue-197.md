# Q-10: a `type (…)` written in a loop: parsed once and built each time, or built once?

## Metadata
- **Status:** answered, relayed 2026-09-29 21:00
- **Priority:** 1 (why: the #197 worker is idle until you answer; one letter answers it)
- **Asked:** 2026-09-29 20:58
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), your first answer is recorded there
- **Branch and worktree:** `issue-197-immediate-moment`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-worker (session f2a5bd3c), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your answer on #197](../answers/Q-8_issue-197.md): "the third example with the loop prit hi iter iter iter because its not parsed three times only once"

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. What happens to a type written inside a loop or function body

Two DESIGN rules describe a stored body with the same words: a type's `run` body is "constructed once per field-type set", and a `type (…)` inside a body is built "each time its node runs". Your answer treats the first as a parse (the `run` body's `immediate` runs per set) and the second as not a parse (the loop parses once). DESIGN has to say which you mean:

- (a) **Parsed once, still built every time.** The `type (…)` is parsed once with the body around it; its `immediate` runs then. It is still a new type at every iteration and every call.
  ```logos
  for 0..3 ( print «a», t := type ( share g := immediate ( print «hi», 1 ) ), print «iter» )
  # hi a iter a iter a iter     (hi before the loop starts)
  mk := fn (t := type ?) -> type ( type ( share e := immediate t ) )
  # error: t has no value when mk's body is parsed
  ```
- (b) **Built once per set of what it reads**, like a `run` body. `immediate` runs at each such build. A type that reads nothing from the loop is the same type every iteration.
  ```logos
  for 0..3 ( print «a», t := type ( share g := immediate ( print «hi», 1 ) ), print «iter» )
  # a hi iter a iter a iter     (hi at the first iteration)
  mk := fn (t := type ?) -> type ( type ( share e := immediate t ) )
  # legal; mk(i32) == mk(i32) becomes true
  ```
  This changes what a type written in a loop is, and #179's expected results.

Recommended by the worker: (a), because it matches your words "not parsed three times only once" and keeps "a new type per call" as it is; only the moment `immediate` runs changes.

**Answer 1:** 
both cases are actually usefull. case a is faster and case b is more expressive. you can do more things with case b. and the existance of run shows that there is a meaningfull difference. but at the same time there isnt often types are created so i would lean towards b since that allows for more general templates for types
