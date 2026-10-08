# Q-1: build the rest of the orchestrator changes now on usage credits, or after the weekly reset?

## Metadata
- **Status:** answered, relayed 2026-09-29 19:28
- **Priority:** 1 (why: it decides what the orchestrator does next; one line to answer)
- **Asked:** 2026-09-29 19:18
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none, this is about the orchestrator itself
- **Branch and worktree:** `dev`, [main checkout](file:///home/o/Personal/Code/LogosLang)
- **Asked by:** Orchestrator, Fable 5.1, context not readable from inside the session
- **Waiting:** the orchestrator keeps relaying and merging while this waits
- **Orchestrator:** Orchestrator
- **Related:**
  - [the skill as it stands](file:///home/o/Personal/Code/LogosLang/.claude/skills/orchestrate/SKILL.md): question files, target copy, in-progress label and the RCA loop are built
  - [the script](file:///home/o/Personal/Code/LogosLang/.claude/skills/orchestrate/orchestrate.sh)

## Question
Built today: question files with a watcher, the copy of `target/` into each new worktree, the `in-progress` label with assignee, the review floor, the RCA loop, CLAUDE_LOG removed. Not built yet, all asked for by you: (1) the skill made global with a small config per repo, so one orchestrator serves unrelated projects; (2) a base branch per issue, so several branches of one repo are managed; (3) a lock on merges, so several orchestrators can run; (4) the generic rules moved from the project CLAUDE.md to the global one, with DESIGN.md kept as the name of every repo's ruling document.

This session now runs on usage credits: they went from 57.80 to 44.26 dollars between 18:40 and 19:20, most of it this session. Items 1 to 4 are about as much work again as today's build.

## Options
1. **Build all four now**: done today, paid from credits.
2. **Build after the weekly Fable reset**: nothing spent; the running loop works as it is, for this one repo.
3. **Build only some now**: name the numbers.

Recommended by the asker: option 2, because nothing running needs them and the three stalled sessions are a better use of the credits.

## Answer
<!-- Write below this line and save. Your words go to the agent verbatim. -->
Answered in chat, 29 September 2026 19:30: "what archive are you talking about of the 8 files? other than that eeryting sounds good and id say go"
Read as: option 1, build all four now.
