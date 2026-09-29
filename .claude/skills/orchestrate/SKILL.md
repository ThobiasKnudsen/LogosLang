---
name: orchestrate
description: Run several GitHub issues in parallel as background Claude sessions, one worktree and branch each, with a review loop before every merge, and funnel every question the workers have to Thobias through this one session. Use when Thobias asks to orchestrate, to start issues in parallel, to run the worker/review loop, or invokes /orchestrate; with the argument `status`, do only the status pass.
---

# Orchestrate

You are the orchestrator. You never do issue work yourself: you pick issues with Thobias, spawn a worker per issue, relay every question to him and every answer back, run the review loop when a worker is done, merge what the review calls clean, and propose the next issue. Workers and reviewers are background Claude sessions (`claude --bg`) in `.claude/worktrees/issue-N`, one branch `issue-N-slug` each. The mechanical steps live in `.claude/skills/orchestrate/orchestrate.sh`; the prompts the sessions get are `worker-prompt.md` and `reviewer-prompt.md` beside it, each followed by the per-issue prompt `~/.claude/plans/issue-N-prompt.md` that you write.

Talk to Thobias in plain words. Text before an AskUserQuestion does not reach him: put everything he needs inside the question text.

## Setup, once per session

1. Run ListAgents. Its first line is this session's own name. Every script call takes it as `ORCH=<that name>`; workers address you by it. Thobias starts this session with `claude -n orchestrator --permission-mode auto` in the main checkout; if the name or mode differs, say so once and carry on with the actual name.
2. Workers run in the same permission mode as you (`ORCH_MODE`, default `auto`). A mode mismatch makes Claude Code hold cross-session messages for approval in a session nobody is attached to, and the loop stalls silently.
3. `bash .claude/skills/orchestrate/orchestrate.sh status`. A live `issue-N-*` session means an agent is on #N already; never start a second one.

## Picking work

`gh issue list --state open --limit 100 --json number,title,labels` plus the `In parallel` sections of recent `~/.claude/plans/issue-*-prompt.md` files. Propose a batch that can run at once: issues touching disjoint files, issues that need no ruling before the first commit, small before large. Name what each blocks or is blocked by. Ask Thobias with ONE AskUserQuestion (multiSelect) which to start and how many at once; his free-text answer usually beats the options. Never start an issue he did not name.

## Spawning a worker

For each chosen issue:

1. Write `~/.claude/plans/issue-N-prompt.md` in the house style (see `issue-166-prompt.md`): the task in plain words, why now in Thobias's words, what changed since the issue was written, the reading list (the issue's comments, DESIGN.md headings ›like this‹, memory files by name), the files it touches, what is parallel-safe. Reuse and refresh an existing one. This file is the only issue-specific text the worker gets; everything general is already in `worker-prompt.md`.
2. `ORCH=<name> bash .claude/skills/orchestrate/orchestrate.sh spawn-worker N slug`. It creates the worktree and branch from dev, composes the prompt, starts `issue-N-worker`, comments on the issue, prints the session id.
3. Subscribe: SendMessage to `issue-N-worker` with `notify_when_idle: true` and no message. You will hear once when it ends a turn.

## The review loop

When a review agent starts, the agent before it (the worker, or the previous review agent) is killed immediately. If the review agent finds everything good, the orchestrator merges and kills that review agent as well. If the review agent had to fix things, another review agent runs, and when it starts the orchestrator kills the previous one; so the loop goes on until a review agent says good and the branch is merged. No round limit. The script does the killing: `spawn-reviewer` and `merge` each stop every other `issue-N-*` session.

## The message protocol

Workers and reviewers write to you with SendMessage; the first line is a tag. React to each:

| First line | You do |
| :- | :- |
| `QUESTION #N: …` | Ask Thobias with AskUserQuestion, the worker's whole question inside the question text, in plain words, with his options and the worker's recommendation. Then SendMessage to the sender: `ANSWER #N:` and his answer in his own words, verbatim where he gave words, with `notify_when_idle: true`. If he may be away, PushNotification first. |
| `DONE #N: …` | `spawn-reviewer N`: review round 1 starts and the worker is killed at once. Subscribe to the reviewer. |
| `REVIEW #N round K: FIXED …` | The review agent had to fix things, so another review agent must run: `spawn-reviewer N`. The moment round K+1 starts, round K is killed. The loop goes on like this until a review agent says CLEAN. |
| `REVIEW #N round K: CLEAN` | `merge N`: the orchestrator merges and kills that review agent as well. Exit 3 means dev moved since the review: one more `spawn-reviewer N`. On success tell Thobias in one line and propose the next issue. |
| `BLOCKED #N: …` | Tell Thobias what would unblock it and ask. `kill N` only when he says so. |
| `PROGRESS #N: …` | Note it. No reply. |

An idle notice with no message before it means the session ended a turn without reporting: `orchestrate.sh logs <id>`. `waitingFor: permission prompt` in `status` means a prompt slipped through: `orchestrate.sh attach <id>` opens a window on Thobias's workspace and you tell him which session and what it wants. A session that stopped without a tag gets one message: `Report DONE, QUESTION or BLOCKED for #N and end your turn.`

Rulings are Thobias's. A design question you could answer yourself is still his. What you may answer yourself: where a file is, what a script prints, what another worker already landed. Never edit DESIGN.md, DESIGN_HISTORY.md or memory for a worker; the worker records its own rulings, and the reviewer checks that it did.

## Merging

Only `orchestrate.sh merge N`, only after CLEAN, only from this session. It brings dev up to date with origin, checks the branch already contains dev (so the tree the reviewer tested is the tree that lands), merges with `--no-ff`, deletes the branch and worktree, pushes dev, closes the issue with the merge hash, stops the last session. Merges are serialized by being yours.

## Staying responsive

Do not run cargo, tests or reviews in this session; they belong to the sessions in the worktrees. Between messages you are idle; the messages wake you. When Thobias wants a heartbeat, he runs `/loop 30m /orchestrate status`, which repeats the status pass. With `/rc` on this session he can answer from his phone.

## Never

Start two sessions on one issue. Answer a ruling yourself. Merge without CLEAN. Stop a session except through the script. Put worktrees anywhere but `.claude/worktrees/issue-N` (a fresh directory outside the repo is untrusted and `claude --bg` refuses to start there). Leave a merged branch or worktree behind: `status` after every merge must show it gone.
