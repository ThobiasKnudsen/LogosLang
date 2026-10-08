# Q-3: #166 round 4 deleted more dead checks: merge now, or pay for a fifth review round?

## Metadata
- **Status:** answered, relayed 2026-09-29 20:09
- **Priority:** 1 (why: #166's merge waits on it, and the #171 rename and the #174 question worker start only after that merge; one word to answer)
- **Asked:** 2026-09-29 20:20
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#166 the one-word node, review of what landed on dev](https://github.com/ThobiasKnudsen/LogosLang/issues/166)
- **Branch and worktree:** `issue-166-review`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-166)
- **Asked by:** Orchestrator, Fable 5.1
- **Waiting:** nothing runs on #166 while this waits, so nothing is spent; the branch stands at 1265c66 with the gate green
- **Orchestrator:** Orchestrator
- **Related:**
  - [round 4's report on the issue](https://github.com/ThobiasKnudsen/LogosLang/issues/166#issuecomment-5895875604): what it fixed and what it declined

## Question
Round 4 of the #166 review reported FIXED. It changed Rust in two commits, and both only delete checks that can never fire since the one-word node: a test whether a node's type word is null, six of them, in `read_kind`, `kind_of`, `Binding::is_storage`, `is_record_type`, the `.operands` read and `declare`'s run. No branch was added. 42 probes give the same output as before, the memory numbers are unchanged, and the full gate is green on the branch merged with today's dev.

The floor you adopted says: a Rust change gets one more round. That would be round 5. The pattern so far: round 1 removed dead null tests, round 2 found four more, round 3 one more, round 4 six more. Each fresh reviewer greps a little wider and finds more of the same class. It is cleanup, not a defect, and a fifth round will likely find a few more and ask for a sixth.

```logos
x := i32 5        # the node is one block: [i32 | 5]
x:type            # the type word is always there, so "is it null?" can never be yes
```

## Options
1. **Merge now on round 4's gate.** The two commits only delete dead branches. I also add one sentence to the floor: a commit that only deletes dead code, with the gate green and the probes unchanged, counts as cosmetic.
2. **Run round 5**, limited to the two Rust commits and the gate. One more session on usage credits.
3. **Merge now, and sweep the rest of that class once** as its own small leaf issue, so no review round hunts for it again.

Recommended by the asker: option 3, because it lands the work today and ends the hunt in one planned pass instead of one paid round at a time.

## Answer
<!-- Write below this line and save. Your words go to the agent verbatim. -->
option 1