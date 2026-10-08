# The orchestrator's folder

This folder is where the repo's orchestrator (a Claude session running the `/orchestrate` skill) and the people it works for talk, and where it keeps its records. One orchestrator runs per repo, on the machine of the person who started it. The skill itself lives outside the repo, in `~/.claude/skills/orchestrate/`, and is the same for every repo: everything this repo's agents must know about this repo is in the repo's `AGENTS.md`, and in this folder's `AGENTS.md` if there is one.

## What the orchestrator does

The owner picks issues; the orchestrator starts one **implementation agent** per issue, a Claude session in its own git worktree and branch. When it is done, a **reviewer** session checks the branch (where work lands by PR, it runs the repo's merge gate and opens the PR) and merges it when the orchestrator says so. No agent asks the owner directly: every question becomes a file in `questions/`.

Any problem an agent meets that its issue does not cover is not filed. It becomes a **problem** in `PWS.json`, and two **why agents** look for its causes without seeing each other's answers; where they disagree, a third agent is asked. The causes with no deeper cause are **root causes**. A **solution agent** then works out a fix for a root cause with the owner, and only the solution the owner chooses becomes an issue.

## Words

- **PWS.json**: the Problem-Why-Solution graph. Ids: `P60` a problem, `W269` a why (a cause), `S70` a solution, the letter and a number (see Ids below).
- **Standing extra**: the third why agent kept on a problem for every disagreement between the first two. **Bottleneck finder**: a helper agent that ranks what holds the project back the most, so the orchestrator starts that work first.
- **Ruling document** (in this repo `DESIGN.md` at the repo root, with `DESIGN_HISTORY.md` beside it): the rulings for this repo's work, each with its reason. A **Ruled** line is a ruling; a **Pending** line is one whose wording must wait; a **Seed** line says what the code does today and is the only line an agent may change.
- **Gate**: the checks a change must pass before it lands. **Receipt**: a merge gate's record that one exact commit passed.
- **`record`**: how the orchestrator's own changes (rulings with no code, questions, `PWS.json`) reach the base as a PR of their own, where work lands by PR.

## What is here

| Path | What it holds | In git |
| :- | :- | :- |
| `AGENTS.md` | if present, every project-specific fact the orchestrator's agents need, each in full, at most 30 lines; the orchestrator puts it into every prompt, and its "For the orchestrator" part is what the orchestrator itself does in this repo | yes |
| `questions/` | open questions to the owner, one file each | yes |
| `answers/` | answered questions | yes |
| `PWS.json` | the graph of problems, the whys behind them and their solutions; written only by `pws.py` | yes |
| `orchestrate.conf` | the repo's facts for the orchestrator: base branch, gate commands, ruling document | yes |
| `orchestrate.local.conf` | this machine's values for the same keys, such as `WORKTREE_ROOT`; they win | no |
| `seen/`, `state/`, `prompts/`, `tmp/`, `why/`, `rca/`, `solve/`, locks | this machine's working files | no, see `.gitignore` |

Only the orchestrator writes this folder, in the main checkout. Where work lands by PR (`MERGE=pr`), its changes reach the base in a PR of its own (made by `record`), except a ruling that comes with code, which is written on the branch that builds it and rides in that code's PR. Beyond that ruling, a branch may change only two things here: Seed lines in the ruling document, and its own issue's question files. The merge refuses anything else.

`bash ~/.claude/skills/orchestrate/orchestrate.sh setup` makes this folder in a new repo from the templates beside the script.

## Answering a question

Open the file, write under each `**Answer N:**` line, and save. Every save wakes the orchestrator; it relays once every question in the file has an answer. Never rename a file you have open. Each question starts with three lines, the decision, the recommendation and why; the detail is below a rule. A file asks only what is the owner's to decide (what the app does for a user, a choice between spec pages, a cost to weigh); what the orchestrator decided itself stands under "Decided", one line each, open to the owner's veto.

The files sort in the order they should be answered. The number before the first `-` is the priority, from 0 (first) to 99 (last), judged for each question alone, so two may share one. VS Code sorts the numbers by value; a plain `ls` sorts them as text (`10` before `9`), and the orchestrator's `questions` command lists the true order. Nothing is renamed when a question is added.

## Ids

An id stays the same through every move. A question's id is `Q` and a number (`Q115`), counted on from the highest in both folders, whoever made it; the Metadata section says who asked. The last part of its file name says who asked and about what: `I82` the implementation agent on issue 82, `R3-I82` round 3 of its review, `S-P89` the solution agent on problem P89 (`S-P87-P97` once two merged), `W269-P42` a why agent on problem P42 asking about the why W269 (`W-P42` about the problem itself), `O` the orchestrator itself. A PWS node's id is its letter and a number (`P60`), counted on the same way; two people making one id is rare and fixed by hand. Older ids (`P-thobias-60`, `Q-72`, `Q-thobias-73`) stay as they are inside old questions and old text.
