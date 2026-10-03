# Q-19: three points your "build it where it can be built" answer leaves open, and the answer to your partial-parse question

## Metadata
- **Status:** answered, relayed 2026-09-29 23:59
- **Priority:** 5 (why: nobody waits; #197's ruling merges without these, which stand as Open lines in DESIGN until you answer)
- **Asked:** 2026-09-29 22:36
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and when a function's or a type's body is built
- **Branch and worktree:** `issue-197-immediate-moment`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-2 (session 70161615, ended), Opus 5.5; recommendations by the orchestrator
- **Waiting:** nobody; review round 3 checks the recorded rule and merges it
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your answer](../history/Q-15_issue-197.md): "when it actually can be built at definition it should be built"
  - [the #199 follow-ups](../history/Q-18_issue-199.md) and [the root-label question](../history/Q-17_issue-203.md): unrelated, answer those first

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

Your rule is recorded: a `fn` body and a type's `run` body are built where they are written when everything the build needs is known there, otherwise where it arrives (a call, the first node of the type).

## Your question: "would it be possible to partially parse some code section and stop and store its state for later?"

Yes, and DESIGN already describes the idea: ›Deferral is authored, and unfinished work is visible‹ says "a node missing an input stays built and moves on the moment the input arrives". But a kept body is only lexed today: each build constructs every node of it from scratch. Partial building would construct at the definition every node that needs nothing the call or the node supplies, keep the rest unbuilt, and let each build copy that half-built body and finish only the rest. Two parts are new: knowing which nodes depend on the parameters, and copying the half-built body per build, since a run never writes the shared graph.

```logos
sq := type ( a := ?, output_type := type ?, share run = ( k := 2 + 3, a * k ), … )
# today:   each new type of `a` builds `k := 2 + 3` and `a * k` again
# partial: `k := 2 + 3` is built once at the definition; each type of `a` builds only `a * k`
```

One consequence: an `immediate` that needs nothing from the parameters would run once at the definition, not at each build.

## 1. A type whose parse always writes the same output type

```logos
sq := type ( a := i32 ?, output_type := type ?, share run = ( immediate ( print «hi», 1 ), a * a ), share parse = ( …, tape[0].output_type = i32, … ) ), print «defined», x := i32 3, x sq
```

A `run` body's build needs the node's output type, and DESIGN has each node's parse write it (›A node's output type is per node, and its parse writes it‹). `sq`'s parse always writes `i32`, but only reading the parse tells.

- (a) **Built at the definition,** by reading what the parse writes: `hi defined 9`.
- (b) **Waits for the first node, as today:** `defined hi 9`. With partial building (point 4) it could move to the definition later.

Recommended: (b), because knowing the output means reading the parse body, which is point 4's partial building.

**Answer 1:** 
but if run will try parse anyways like all other cases and then hits ? somewhere like the other cases then immediate should be runned in place when parsed and then its stops at a*a. so would say a. but run doesnt run the code when parsed, it runs it later. while parse actually runs the code isnide its body. but since the run body basically returns 1 before a*a this is a bad example and would be bad code in practice since a*a is never reached when runned. 

## 2. A type that declares no output type

```logos
sq2 := type ( a := i32 ?, share run = ( immediate ( print «hi», 1 ), a * a ) ), print «defined», x := i32 3, x sq2
```

Its field is typed and it has no output type, so nothing is left to wait for.

- (a) **Built at the definition:** `hi defined …`. This reverses your first answer's case 2, where `sq2` printed nothing until `x sq2`.
- (b) **A missing output type counts as unknown, so it waits:** `defined hi …`, as your first answer said.

Recommended: (a), because it is what "when it actually can be built at definition it should be built" gives.

**Answer 2:** 
a

## 3. A `type (…)` in a loop body that reads nothing of the loop

```logos
for 0..3 ( print «a», t := type ( share g := immediate ( print «hi», 1 ) ), print «iter» )
```

- (a) **Built where written,** since everything is known: `hi a iter a iter a iter`.
- (b) **Built at its first run,** as your example an hour earlier showed: `a hi iter a iter a iter`.

Recommended: (a), for the same reason as point 2.

**Answer 3:** 
a

## 4. Should partial building become an issue?

- (a) **Yes, as an idea for later:** the explanation and example above go into a new issue; nothing is built now.
- (b) **Not now.**

Recommended: (a), because you called it "really nice", and an issue keeps the idea with its example.

**Answer 4:** 
a
