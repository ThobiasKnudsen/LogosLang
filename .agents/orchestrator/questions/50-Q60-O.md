# Q-60: When a root merges, should the issues it unblocks be rechecked in the same step? (management)

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. Recheck at each merge?

**What happened.** #192 and #215 merged on 1 October, but 15 open issues still said "Blocked by #192" (or #215). The merge step never looks at the issues a root unblocks. A helper reran each one's programs on dev tonight:
- 4 were already fixed and stood open: #191, #170, #94, #68. I checked them again and closed them.
- 11 now wait on something else, mostly #82. Their first lines are updated.
- On the way it found two new problems (P56, P57), and that #194 and #228 fail for one reason, which #82's slice can take.

So a stale first line hides fixed issues, and it also sends the next worker to the wrong issue.

- (a) **Recheck at each merge.** `merge N` lists the open issues whose first line names #N. A helper reruns their programs on the new dev. A fixed one is closed with its outputs; one still failing gets its new blocker in its first line. Cost: one helper run per merged root. Tonight's 15 took 24 minutes and about 350k tokens; a usual merge unblocks fewer.
- (b) **Recheck only when an issue is picked.** It is cheaper, but the list stays wrong until then, and so do the batches I propose to you from it.

Recommended: (a). I would add the list to `merge` and one line to SKILL.md's ›Merging‹.

**Answer 1:** 

## Metadata
- **Status:** open
- **Priority:** it decides whether the issues #82 unblocks are rechecked when it merges, which comes next; one letter
- **Asked:** 2026-10-02 00:09
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none (the orchestrate skill's merge step)
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2)
- **Waiting:** nothing; I go on as today until you answer
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [SKILL.md](file:///home/o/.claude/skills/orchestrate/SKILL.md), ›Merging‹; CLAUDE.md's Issue rules: "Every other open issue says in its first line what it waits for"
