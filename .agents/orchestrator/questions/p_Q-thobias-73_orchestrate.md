# Q-thobias-73: Should LogosLang's agent folders move out of the repo, as AI-RCA's did?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. Where should LogosLang keep the folders its agents work in?

Each agent the orchestrator starts works in its own copy of the repo, called a worktree, on its own branch. Today LogosLang keeps these copies inside the repo, in `.agents/worktrees/` (for example `.agents/worktrees/issue-82`).

On 4 October 2026 you ruled for AI-RCA: "go with the global folder if trust works". You ruled this because copies inside AI-RCA's repo confused that project's own tools: `npm run lint` checked all 27 copies, and Next.js mistook the main checkout for a copy's project folder. The condition was that Claude Code must still trust a copy kept outside the repo. I tested that the same night, starting a session the way the orchestrator does: a copy of AI-RCA in `~/.claude/worktrees/ai-rca/` started and answered, while a plain empty folder beside it was refused with "Workspace not trusted". So AI-RCA now keeps its copies in `~/.claude/worktrees/ai-rca/`. The orchestrator script reads the place from a new setting, `WORKTREE_ROOT`, in each repo's settings file.

LogosLang has not shown these problems. Its tools (cargo, rg) skip the copies, because git ignores that folder. Copies already open inside the repo would finish where they are under every option below.

- (a) **Keep LogosLang's copies inside the repo.** Nothing changes for LogosLang. Each repo chooses its own place with the setting, so the two repos work differently.
- (b) **Move LogosLang's copies to `~/.claude/worktrees/logoslang/` too.** Both repos then work the same way. The setting stays, so a future repo could still choose to keep its copies inside.
- (c) **Make the global folder the rule for every repo, and delete the setting.** Every repo, including new ones, keeps its copies in `~/.claude/worktrees/<project>/`. The script gets simpler because there is one place and no choice. Your global CLAUDE.md's Branch rules say `.agents/worktrees/issue-N`, so that line would need your edit (you edit that file yourself).

Recommended: (c), because one place for every repo is the simplest rule, the test showed it works, and it avoids the tool problems before they appear in a new repo.

**Answer 1:** 

## Metadata
- **Status:** open
- **Priority:** last in the order: no agent waits for it and it blocks no work; AI-RCA has already moved; one letter answers it.
- **Asked:** 2026-10-04 01:05
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none; a question about how the orchestrator works
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2) (session 79afb672), claude-opus-5-5
- **Waiting:** nothing waits for this answer
- **Related:**
  - [orchestrate.sh](file:///home/o/.claude/skills/orchestrate/orchestrate.sh): `use_repo` reads `WORKTREE_ROOT`; `wt_of` still finds copies under the earlier folders
  - [SKILL.md](file:///home/o/.claude/skills/orchestrate/SKILL.md): the repo paragraph names the setting and your AI-RCA ruling; the Never list says how the test went
