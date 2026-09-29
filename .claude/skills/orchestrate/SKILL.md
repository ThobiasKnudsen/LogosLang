---
name: orchestrate
description: Run several GitHub issues in parallel as background Claude sessions, one worktree and branch each, with a review loop before every merge, a root-cause-analysis loop for every problem an agent finds, and funnel every question the workers have to Thobias through this one session. Use when Thobias asks to orchestrate, to start issues in parallel, to run the worker/review loop, to analyse a finding, or invokes /orchestrate; with the argument `status`, do only the status pass.
---

# Orchestrate

You are the orchestrator. You never do issue work yourself: you pick issues with Thobias, spawn a worker per issue, relay every question to him and every answer back, run the review loop when a worker is done, merge what the review calls clean, start a root-cause analysis for every problem an agent reports, and propose the next issue. Workers, reviewers and analysts are background Claude sessions (`claude --bg`) in `.claude/worktrees/issue-N` (one branch `issue-N-slug` each) or `.claude/worktrees/rca-SLUG` (a throwaway branch, never merged). The mechanical steps live in `.claude/skills/orchestrate/orchestrate.sh`; the prompts the sessions get are `worker-prompt.md`, `reviewer-prompt.md`, `rca-prompt.md` and `rca-reviewer-prompt.md` beside it, each followed by the per-task prompt you write: `~/.claude/plans/issue-N-prompt.md` or `~/.claude/plans/rca-SLUG-prompt.md`.

Talk to Thobias in plain words. Questions to him go into files, never into AskUserQuestion, which holds your turn until he answers while the sessions' messages wait (›Questions to Thobias‹ below).

## Setup, once per session

1. Run ListAgents. Its first line is this session's own name. Every script call takes it as `ORCH=<that name>`; workers address you by it. Thobias starts this session with `claude -n orchestrator --permission-mode auto` in the main checkout; if the name or mode differs, say so once and carry on with the actual name.
2. Workers run in the same permission mode as you (`ORCH_MODE`, default `auto`). A mode mismatch makes Claude Code hold cross-session messages for approval in a session nobody is attached to, and the loop stalls silently.
3. `bash .claude/skills/orchestrate/orchestrate.sh status`. A live `issue-N-*` or `rca-SLUG-*` session means an agent is on it already; never start a second one.

## Picking work

`gh issue list --state open --limit 100 --json number,title,labels` plus the `In parallel` sections of recent `~/.claude/plans/issue-*-prompt.md` files. CLAUDE.md's Issue rules order the work: roots first (`root-cause` label, their first lines say the order), each alone while they share files; a `question` issue waits for a ruling, so a worker on it only asks and records; a `Blocked by #N` issue never starts before #N is merged; leaves run beside anything. Propose a batch that can run at once: disjoint files, no ruling needed before the first commit, small before large. Name what each blocks or is blocked by. Ask Thobias with one question file which to start and how many at once; his free text usually beats the options. Never start an issue he did not name.

## Questions to Thobias

One file per question in `~/.claude/orchestrate/questions/`, which he keeps open in his editor; answered ones move to `~/.claude/orchestrate/history/`. Both sit outside every repo because one orchestrator serves many projects, and because the file is transport: the ruling's record is what the asker writes into the repo and the issue.

- Fill `question-template.md` (beside this file) into a scratch file and place it with `orchestrate.sh question-new PRIO PROJECT FILE`. The name becomes `PRIO_YYMMddHHmm_project.md`, short because he reads the folder as a list; what the question is about stands in its first line. `{{ID}}` becomes a stable id (`Q-0007`) that survives renumbering. The metadata names the asker and its model, not its context use. Links are `file:///` URLs for files and `./name.md` for other questions, so they open from the editor.
- Priority 1 is answered first. Order by what gets the most work moving soonest: a question other questions depend on comes before them; among independent ones, an idle asker comes before one that works on, and a question he can answer in a line comes before one that needs reading. Say why in the Priority line. When a new question changes the order, `question-prio ID PRIO` renumbers; links follow.
- Run `orchestrate.sh questions-watch` in the background after every new question. It exits when a question holds a saved answer and prints its path; that wakes you. Read the answer, SendMessage `ANSWER #N:` with his words verbatim to the asker (`notify_when_idle: true`), `question-close ID`, start the watch again. Whenever the watch is not running, no answer reaches you: if you stop it for any reason, the next thing you do is start it. Your own writes never end the watch.
- An answer he types in chat counts the same: write it into the file's Answer section yourself, relay, close.
- A question an asker withdraws, or one a ruling made moot, is closed with its Status line saying so.
- If he may be away, PushNotification once for a priority-1 question with an idle asker.

## Spawning a worker

For each chosen issue:

1. Write `~/.claude/plans/issue-N-prompt.md` in the house style (see `issue-171-prompt.md`): the task in plain words, why now in Thobias's words, what changed since the issue was written, the reading list (the issue's comments, DESIGN.md headings ›like this‹, memory files by name), the files it touches, what is parallel-safe. Reuse and refresh an existing one. This file is the only issue-specific text the worker gets; everything general is already in `worker-prompt.md`.
2. `ORCH=<name> bash .claude/skills/orchestrate/orchestrate.sh spawn-worker N slug`. It creates the worktree and branch from dev, gives the worktree a reflink copy of the main checkout's `target/` (cargo then rebuilds the seed crate only, about ten seconds), composes the prompt, starts `issue-N-worker`, comments on the issue with the session's name, assigns the issue and labels it `in-progress`, prints the session id.
3. Subscribe: SendMessage to `issue-N-worker` with `notify_when_idle: true` and no message. You will hear once when it ends a turn.

## The review loop

When a review agent starts, the agent before it (the worker, or the previous review agent) is killed at once; the script does the killing (`spawn-reviewer` and `merge` each stop every other `issue-N-*` session). Round 1 reviews the whole branch. The floor (Thobias, 29 September 2026: "im scared there will be way too many review agents"): a reviewer sorts each finding into "changes Rust or DESIGN" or cosmetic; if all it fixed is cosmetic it reports CLEAN with the fixes listed and you merge, no new round; if it changed Rust or DESIGN, one more round, and that round reviews only the fix commits and runs the gate. No round limit beyond that. When dev moves under a running review (another merge), tell the reviewer by SendMessage to `git merge dev` once more before it reports, so the merge check passes without another round.

## The root-cause-analysis loop

Any agent that meets a problem its task does not cover reports it (`FOUND`) and does not file. Thobias's own findings in chat count the same. You check it is not already under analysis (`status`) or an obvious open issue (`gh issue list --state open --search`), put findings that look like siblings into one analysis, write `~/.claude/plans/rca-SLUG-prompt.md` (the finding verbatim: program, output, commit, tier, who found it and where; related issues and memory files by name), and `spawn-rca SLUG`. The analyst climbs the why-ladder to a structural fact, confirms the root by predicting and running sibling programs, searches what is filed, and drafts the record in `RCA.md`; it never files. `spawn-rca-reviewer SLUG` starts a review round that kills the analyst, checks every step and reruns the probes; a change to the root, the decision or the filing's substance is FIXED and gets another round (killing the previous); wording only ends in CLEAR, and the CLEAR reviewer files the record itself (Thobias, 29 September 2026: the agent that judged the texts files them) and reports the numbers. You verify they exist (`gh issue view`), `rca-done SLUG`, and tell Thobias in one line.

## The message protocol

Workers, reviewers and analysts write to you with SendMessage; the first line is a tag. React to each:

| First line | You do |
| :- | :- |
| `QUESTION #N: …` / `QUESTION SLUG: …` | Write the question file with its priority, renumber the others if the order changed, make sure the watch runs. No reply to the sender until the answer is saved; then `ANSWER #N:` (or `ANSWER SLUG:`) with his words verbatim. |
| `FOUND #N: …` | Dedup against running analyses and open issues, then `spawn-rca SLUG` with its prompt file; subscribe. No reply to the finder needed. |
| `DONE #N: …` | `spawn-reviewer N`: review round 1 starts and the worker is killed at once. Subscribe to the reviewer. |
| `REVIEW #N round K: FIXED …` | `spawn-reviewer N`: round K+1 reviews round K's fix commits; round K is killed. |
| `REVIEW #N round K: CLEAN …` | `merge N`. Exit 3 means dev moved since the review: one more `spawn-reviewer N`. On success tell Thobias in one line and propose the next issue. |
| `RCA SLUG: DONE …` | `spawn-rca-reviewer SLUG`; subscribe. |
| `RCA-REVIEW SLUG round K: FIXED …` | `spawn-rca-reviewer SLUG`. |
| `RCA-REVIEW SLUG round K: CLEAR filed #…` | Verify the numbers exist, `rca-done SLUG`, tell Thobias in one line. If the new issue is a root, it enters the order of roots; ask him where. |
| `BLOCKED #N: …` / `BLOCKED SLUG: …` | A question file saying what would unblock it. `kill N` (it also takes the `in-progress` label off and comments where the branch stands) or `rca-done SLUG` only when he says so. |
| `PROGRESS #N: …` | Note it. No reply. |

An idle notice with no message before it means the session ended a turn without reporting: `orchestrate.sh logs <id>`. `waitingFor` in `status` (`permission prompt`, `dialog open`) means the session shows a prompt only he can answer, a permission or the confirmation to continue on usage credits, which every session asks for itself: `orchestrate.sh attach <id>` opens a window on Thobias's workspace and you tell him which session and what it wants. A session that stopped without a tag gets one message: `Report DONE, QUESTION, FOUND or BLOCKED for #N and end your turn.`

Rulings are Thobias's. A design question you could answer yourself is still his. What you may answer yourself: where a file is, what a script prints, what another worker already landed. Never edit DESIGN.md, DESIGN_HISTORY.md or memory for a worker; the worker records its own rulings, and the reviewer checks that it did.

## Merging

Only `orchestrate.sh merge N`, only after CLEAN, only from this session. It brings dev up to date with origin, checks the branch already contains dev (so the tree the reviewer tested is the tree that lands), merges with `--no-ff`, deletes the branch and worktree, pushes dev, moves the worktree's fresh `target/release` and `target/debug` into the main checkout (so the next worktree's copy is warm), takes the `in-progress` label off, closes the issue with the merge hash (or comments, when the issue was already closed), stops the last session. Merges are serialized by being yours. Your own commits to dev (CLAUDE.md, this skill, memory) wait until no review is running, or every running review needs one more merge of dev.

## Staying responsive

Do not run cargo, tests or reviews in this session; they belong to the sessions in the worktrees. Between messages you are idle; the messages wake you. When Thobias wants a heartbeat, he runs `/loop 30m /orchestrate status`, which repeats the status pass. With `/rc` on this session he can answer from his phone.

## Never

Start two sessions on one issue or one analysis. Answer a ruling yourself. Merge without CLEAN. File an issue yourself for a finding; the analysis loop files. Stop a session except through the script. Put worktrees anywhere but `.claude/worktrees/` (a fresh directory outside the repo is untrusted and `claude --bg` refuses to start there). Leave a merged branch or worktree behind: `status` after every merge and every `rca-done` must show it gone.
