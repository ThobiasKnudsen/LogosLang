# Q-46: After your Q-44 answers 5 and 6: is a `type (…)` in a function still built where it is written when it can be, and does a loop's type change too?

## Metadata
- **Status:** answered, relayed 2026-10-01 02:52
- **Priority:** 0 (why: #197's reviewer is idle until these are answered, and recording answers 5 and 6 waits for them; one letter each)
- **Asked:** 2026-10-01 02:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-3 (session fef213ff), Opus 5.5
- **Waiting:** the reviewer is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-44_issue-197.md](file:///home/o/.claude/orchestrate/history/Q-44_issue-197.md): Q-44, your answers 5 and 6, which these points come from
  - [Q-38_issue-197.md](file:///home/o/.claude/orchestrate/history/Q-38_issue-197.md): Q-38, your answer 1, which point 1 weighs against answer 5
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›A `type (…)` inside a body is built once per set of the values it reads‹, the rule both points change

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Your Q-44 answer 4 ("a") needs no DESIGN change, and the reviewer records it on #197. Answers 5 and 6 ("a new type at every call", because "there is nothing defined to do minting of anything a function runs") are not recorded yet: they give different outputs from your Q-38 answer 1 of the same day, and they leave loops open. Both decide the rule's text and its heading. The outputs below are from the branch's release build, interpreted.

#197 is when `immediate` runs and where a body is built; #219 is building a body in part where it is written, the seed change after #197.

## 1. A `type (…)` in a function whose reads all hold their values where it is written: built there, or a new type at every call?

Your Q-38 answer 1 ("a", 1 October) built it where it is written: `bump defined`. Your answer 5 makes a type in a function body a new type at every call. For this program the two give different outputs:

```logos
bump := fn () -> i32 ( print «bump», 1 ),
mut t := type ?, t = i32,
f := fn () -> type ( type ( share e := t, share g := bump() ) ),
print «defined», a := f(), b := f(), a == b
```

- (a) **A new type at every call, never built where it is written:** `defined bump bump false`. Q-38 answer 1 is reversed. This is answer 5 with no exception, and it fits your Q-44 answer 3 ("constructed every time the function runs just like if the body was ( t ) or ( 1 + 2 )"). The seed prints this today.
- (b) **Built once where it is written, and every call gets that one type:** `bump defined true`. Q-38 answer 1 stands, as an exception to answer 5 for a type that needs nothing from the call. That is a mint over "nothing" that nothing writes out, where your answer 6 says minting "you would have to somehow explisitly define".

Recommended: (a). It is one rule with no exception, matching your answers 3, 5 and 6. Its cost: a function that returns a fixed type makes a new one at every call, unless its body says what it mints on (an Open line, as you asked). An `immediate` in the type runs where it is written either way (Q-29: `mk2` prints `hi defined`).

**Answer 1:** 
a

## 2. A `type (…)` in a loop body outside any function: unchanged, or a new type every time the body runs, like a function body?

Your 29 September "b" was asked over a loop and `mk` together: once per set of the values it reads. Your Q-19 answer 3 ("a") built a loop's type that reads nothing of the loop where it is written, so it is the same type every time round. Your answer 6 speaks of "anything a function runs".

```logos
bump := fn () -> i32 ( print «bump», 1 ),
for 0..3 ( t := type ( share g := bump() ), print «iter» )
```

- (a) **Unchanged outside functions:** `bump iter iter iter`. It is built where it is written when its reads hold their values there, otherwise once per set of what it reads; a type over the loop counter makes one type per value.
- (b) **Like a function body, a new type every time the loop body runs:** `bump iter bump iter bump iter`. A type is then made each time its body runs, like `( 1 + 2 )`, in every body, and nothing is minted unless the body says what it mints on. The seed prints this today.

Recommended: (b). It is one rule for every body, and an `immediate` in the type still runs once where it is written (`for 0..3 ( print «a», t := type ( share g := immediate ( print «hi», 1 ) ), print «iter» )` still prints `hi a iter a iter a iter`). Its cost: Q-28 point 2's loop example becomes `bump iter bump iter bump iter`, and "the same type every time round" goes.

If both answers are the recommended ones, the rule is back to its 25 September heading, ›A `type (…)` inside a body is built each time it runs‹, plus what was ruled since: built in part where it is written (an `immediate` runs there or is the error there), and its `share` lines made at each build. #197's main seed symptom (`mk(i32) == mk(i32)` false) then becomes the rule. What is left for #197's seed change is `sq`/`sq2` built at the definition and #219's building in part; the issue's title and slices are rewritten when this is recorded.

**Answer 2:** 
b
