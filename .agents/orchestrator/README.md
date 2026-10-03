# The orchestrator's folder

This folder is where the repo's orchestrator (a Claude session running the `/orchestrate` skill) and the people it works for talk, and where it keeps its records. One orchestrator runs per repo, on the machine of the person who started it. The skill itself lives outside the repo, in `~/.claude/skills/orchestrate/`.

## What is here

| Path | What it holds | In git |
| :- | :- | :- |
| `questions/` | open questions to the owner, one file each | yes |
| `history/` | answered questions | yes |
| `PWS.json` | the graph of problems, the whys behind them and their solutions; written only by `pws.py` | yes |
| `orchestrate.conf` | the repo's facts for the orchestrator: base branch, gate commands, ruling document | yes |
| `orchestrate.local.conf` | this machine's values for the same keys, such as `MEMORY_DIR`; they win | no |
| `seen/`, `state/`, `prompts/`, `tmp/`, `why/`, `rca/`, `solve/`, locks | this machine's working files | no, see `.gitignore` |

Only the main checkout writes this folder, on the base branch. A branch may change only two things here: Seed lines in the ruling document, and its own issue's question files. The merge refuses anything else.

`bash ~/.claude/skills/orchestrate/orchestrate.sh setup` makes this folder in a new repo from the templates beside the script.

## Answering a question

Open the file, write under each `**Answer N:**` line, and save. Every save wakes the orchestrator; it relays once every question in the file has an answer. Never rename a file you have open.

The files sort in the order they should be answered. The part before the first `_` is the priority: lowercase letters compared as text, so `b` comes before `bm`, which comes before `c`. A new question gets a priority between two others, and no other file is renamed.

## Ids

An id stays the same through every move. Ids made since October 2026 carry their maker's short name, `Q-thobias-73` for a question and `P-thobias-60` for a PWS node, so two people never make the same id. Your orchestrator's watch and its `questions` list show only your ids and the old ones. Set your name once per machine:

```
git config --global orchestrator.user <name>
```

Use lowercase letters and digits. Old ids without a name (`P59`, `Q-72`) stay as they are.
