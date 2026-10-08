# Q-2: CLAUDE_LOG is deleted; keep or delete the 8 files that open issues cite?

## Metadata
- **Status:** answered, relayed 2026-09-29 19:39
- **Priority:** 2 (why: nothing waits on it; one word to answer)
- **Asked:** 2026-09-29 19:18
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none
- **Branch and worktree:** `dev`, [main checkout](file:///home/o/Personal/Code/LogosLang)
- **Asked by:** Orchestrator, Fable 5.1, context not readable from inside the session
- **Waiting:** nothing waits
- **Orchestrator:** Orchestrator
- **Related:**
  - [the archive folder](file:///home/o/.claude/orchestrate/archive/LogosLang-CLAUDE_LOG): the 8 items, 1.3 MB
  - [Review_2026-09-09_seed.md](file:///home/o/.claude/orchestrate/archive/LogosLang-CLAUDE_LOG/Review_2026-09-09_seed.md): cited as "full list with evidence" by #82, #99 and twelve more open issues

## Question
CLAUDE_LOG was never in git (it was gitignored), so deleting it is permanent. The 173 session logs are deleted, the folder is gone, and every instruction to write logs is out of the prompts, CLAUDE.md, README and the faithfulness skill. Eight items were not session logs: four review reports, two handoffs and two folders of DESIGN drafts. Sixteen open issues point at the review reports as their evidence, so I moved those eight out of the repo into the archive folder instead of destroying them.

## Options
1. **Keep the archive**: the issues' evidence stays readable on this machine.
2. **Delete the archive too**: `rm -r ~/.claude/orchestrate/archive/LogosLang-CLAUDE_LOG`; the issues keep their own text, the long evidence lists are gone.

Recommended by the asker: option 1, because it costs 1.3 MB and cannot be undone otherwise.

## Answer
option 2. i dont think these things are that significant. and things should be configured to not store things in CLAUDE_LOG
