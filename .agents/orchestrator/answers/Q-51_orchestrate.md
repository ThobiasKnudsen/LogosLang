# Q-51: Should only the orchestrator write DESIGN.md, right when you rule, and every agent only read it?

## Metadata
- **Status:** answered "a" 2026-10-01 03:14; built the same day. The orchestrator writes every ruling on the base before relaying it (SKILL.md, ›Questions to Thobias‹). Agents change only Seed lines, which `merge` checks for branches begun since. Solution agents read a detached checkout.
- **Priority:** 1 (why: you raised it in chat; how your next ruling is recorded waits for it; one letter)
- **Asked:** 2026-10-01 03:15
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none, a question about how the orchestration works
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2), Opus 5.5
- **Waiting:** nothing stops; the next ruling is recorded the old way until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [SKILL.md](file:///home/o/.claude/skills/orchestrate/SKILL.md): how rulings are recorded today
  - [Q-46_issue-197.md](file:///home/o/.claude/orchestrate/history/Q-46_issue-197.md): Q-46, the ruling the W9 agent did not see

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. Who writes a ruling into DESIGN.md?

Your idea in chat: "maybe it would be better to have your agents only have read only access to DESIGN are rather you updating DESIGN right when new rulings are made?"

**Today:** the agent that asked writes your ruling into DESIGN.md on its own branch. The ruling reaches `dev` only when that branch merges, after its review rounds: hours. Meanwhile every other agent reads `dev`'s DESIGN.md and sees the old rule.

**What it cost today:** you answered Q-44 point 6 around 01:00 ("there is nothing defined to do minting of anything a function runs"). It sits on #197's branch and is still not on `dev`. The W9 solution agent read `dev`, where DESIGN still said:

```logos
mk := fn (t := type ?) -> type ( type ( share e := t ) ),
mk(i32) == mk(i32)    # dev's DESIGN: true. Your answer: false
```

It called `false` a bug and built its whole question on that. I caught it and it redid the question (now Q-50).

- (a) **Your idea: I write every ruling into DESIGN.md on `dev` the moment you answer** (a Ruled line with your words, a History line for what it replaces), commit and push, and tell every running agent to merge `dev`. No agent edits DESIGN.md, with one exception: a branch still changes a **Seed** line (what the seed does) together with the code that makes it true, since that is a fact about the code, not a ruling. `merge` refuses a branch that changes anything else in DESIGN.md. The agent that asked checks my record against your words when it merges `dev`, so every record gets a second pair of eyes. A solution agent sends me the wording you approved instead of committing it. Cost: my context grows with each record; a big one (#197's renamed a heading and its 10 pointers) I may hand to a short helper session.
- (b) **(a), but a short helper session writes each record**, I check its diff against your words and merge it at once. My context stays small; one more session per answer (about as cheap as a solution agent's first round), and a few minutes slower.
- (c) **Keep today's way, plus a "ruled, not yet in DESIGN" list** every agent reads first. Cheapest, but then rules live in two places, which is what the faithfulness rules forbid.

Recommended: (a). It is the simplest: one writer, nothing new to run, your ruling on `dev` within minutes. Rulings already recorded on #192's and #197's branches land when those merge.

**Answer 1:** 
a
