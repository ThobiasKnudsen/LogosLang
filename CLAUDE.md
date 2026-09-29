# Follow Elon Musks rules faithfully:
1. **Question every requirement:** Find out who exactly created the requirement and never just accept that it's from "the safety or legal department". Make the requirements less dumb.
2. **Delete parts or processes:** Delete as many parts or steps as you can. Musk's rule of thumb: If you aren't forced to put back at least 10% of what you deleted, you didn't delete enough.
3. **Simplify and optimize:** Only do this after you have completed steps 1 and 2, because a massive waste of time is optimizing something that shouldn't exist in the first place.
4. **Accelerate cycle time:** Speed up the process.
5. **Automate:** Only automate the process after the first four steps are done.

# Behaviour rules
- Always answer Thobias in simple, easily digestible text, with a Logos code example wherever one fits. An example is what makes a point easy to understand.
- Before starting work that takes a long time, say which other open issues can run in parallel (disjoint files, no ruling needed first), so Thobias can start other agents on them.
- Record everything important about an issue on the issue itself, so the next agent that picks it up has all it needs: each ruling as `Q: … / A (Thobias, date): …`, each landed slice with its commit, what was left out and why, what a reviewer should probe.

# Issue rules (root causes first):
- Reason (Thobias, 29 September 2026): a week's batch of small issues were all symptoms of a few structural causes (one rule written twice, once for the interpreter and once for the compiler; a Rust branch per node kind where DESIGN says the record decides; a mechanism DESIGN had superseded). Fixing symptoms one by one added arms and copies, and every sibling case became a new issue. "root causes should always be fixed first before any other problems."
- **Search first.** A problem you meet (a failing probe, a review finding, a bug seen while building something else) is searched for before anything is filed: `gh issue list --state open --search "…"` with two or three spellings. An existing issue gets the new symptom as a comment: the Logos program, its output, the commit it was seen on.
- **Find the root before filing.** Ask why the symptom happens, then why that answer holds, and again, until the answer is a fact about the structure of the seed or the spec rather than a mistake at one line: a rule implemented in two places, a `match`/`if` on node kind where DESIGN's rule has one case, a hand-kept list of kinds, a mechanism DESIGN has superseded, a rule DESIGN does not have yet. That is the root. Test it: if it were fixed, would this symptom and its sibling cases become impossible (the named value against the unnamed one, the interpreter against the compiled tier, one type against another), and would the seed be closer to DESIGN? One root has many symptoms, so read the open `root-cause` issues before deciding the root is new. A problem whose why ends at one wrong line has no root beyond itself and is filed as it is.
- **File at the root.** A new root gets an issue with the `root-cause` label: the structural fact, the symptoms it explains (each with a Logos repro), the DESIGN rule the fix must satisfy, and the slices. A symptom gets its own issue only when it needs its own record (its own repro, test or ruling), and then its first line is `Blocked by #N`, the root.
- **Link every issue.** A `root-cause` issue is blocked by nothing; if it turns out to wait for another issue, that one is the root and the label moves. Every other open issue says in its first line what it waits for: `Blocked by #N`, a root or an ordering dependency (a rename, a merge). An open issue with neither the label nor a `Blocked by` line has not been examined: whoever touches it finds its root and edits it (`gh issue edit`, `gh issue comment`). When a root is filed or found, comment `Blocked by #root` on each symptom it explains and list the symptoms on the root. This is how the work is ordered, so it is never left for later.
- **Order the work by the links.** Roots first, in dependency order, each alone while they share files. A symptom is fixed before its root only when it blocks something now (a crash on a path in use, a release), then as the smallest change, naming the root in the issue and the commit message so the change is deleted with the root's fix, never kept as a second mechanism. Never fix a symptom in code the root's fix deletes.
- **Fix at the root, and review for it.** A fix that adds an arm to a `match` on node kind, a second copy of a rule, a special case or a list entry is a symptom fix. A fix that removes a branch, a copy or a hand-kept list so that one rule reads the record is a root fix. Every review says which one landed; an added arm is a new symptom, recorded on the root's issue.

# Faithfulness protocol (spec-governed code):
- DESIGN.md is the ruling document. Issues, plans, memories, old comments and existing code are downstream and may be stale. Never implement from a downstream source alone.
- Before implementing anything spec-governed, quote the exact DESIGN.md passage(s) that license it, in the plan or the commit message. No quote → stop and ask.
- If any two sources disagree — DESIGN vs sketch, DESIGN vs an issue, one DESIGN section vs another — STOP and surface the conflict as a blocking question, with both quotes. Never silently pick a side, even if one side is newer or was written by Thobias: staleness is invisible from inside a session.
- When a conflict is ruled on, or a design is rejected in conversation, record it in the same session: in DESIGN.md, at the rule it touches (a **Ruled** line with date, who and reason; a **Rejected, to stay rejected** line; or a new `###` rule), and what it supersedes as a **History** line in DESIGN_HISTORY.md under the same heading or, if spec wording must wait, as an explicit pending-spec-edit in the session log AND auto-memory. An unrecorded decision is a future bug.
- DESIGN.md is one rule per `###` heading (since 27 September 2026). Point at a rule by its heading, `«### like this»`, never by line number; a `DESIGN.md l.N` in a Source bullet, an issue or a memory means line N of the paragraph form at git e75bcdc.
- Before starting work in a spec area, run /faithfulness-audit.
- All rulings MUST have a good reason for existing, otherwise I will forget later why I chose what I chose and change the rule. And when something is superseded it also MUST say why.

# Comment rules (as few comments as possible; only what is actually important):
- A comment says what the code cannot: a one-line WHY at a spot a reader would not guess, an invariant (the SAFETY line on an unsafe block, a byte layout, what a slot holds), a bare pointer to the DESIGN.md rule by its heading, or a two-to-four-line module header saying what the file is. Nothing else.
- No ruling history in code: no dates, no "ruled", "superseded", "amended", no issue numbers, no quotes from DESIGN.md. That story lives in DESIGN.md, the commit message and CLAUDE_LOG. The one exception is a known stand-in, marked with one phrase: "stand-in for #N".
- No comment that restates the line below it, and no doc comment that only rephrases the item's name. If the name says it, the comment goes.
- When in doubt, leave it out. A reader who needs more reads DESIGN.md or the git log, which is where the reasons are kept.
- Reason for these rules: comments were 38% of the seed and were where staleness lived; every sentence that repeats a ruling is one more sentence that goes wrong when the ruling changes.

# Release rules (a version tag is one-way):

**Release tags are immutable. Pushing one is irreversible, so a tag that fails CI burns that version number forever.**

The `freeze-release-tags` ruleset blocks both `deletion` and `update` on every `refs/tags/v*`. A pushed tag cannot be moved or removed by anyone short of editing the ruleset in repo settings. This is deliberate: a released version must always mean one commit. The consequence is that `git push origin vX.Y.Z` is a one-way door — if the release workflow then fails, the tag stays pinned to the broken commit, the version is spent, and the next attempt must use a **new, strictly higher** version. (This is exactly how v0.0.3 was lost: the `gate` job died on a first-release-only bug, and v0.0.4 shipped the identical tree.)

A spent version also freezes its docs. `docs-check.sh validate` treats any `docs/vX.Y.Z/` at or below the newest tag as frozen and fails the PR on any add, modify, or delete inside it — so the abandoned version's docs folder can no longer be removed either, and its replacement must be a copy under the new version.

Before pushing any version tag:

1. `cargo test --release` and `cargo test` are green.
2. `bash .github/scripts/docs-check.sh validate` passes.
3. `bash .github/scripts/docs-check.sh release vX.Y.Z` passes — this is the exact check the `gate` job runs, and the in-progress `docs/vX.Y.Z/` folder must be named for the version being released.
4. `bash .github/scripts/docs-check.test.sh` passes (the guard's own self-test).
5. The tag is on `main`, and `main` is already merged and green. `main` is protected: it takes a PR with 3 required checks (the docs guard's `test` and `validate`, and `ci`'s `rust`: fmt, clippy with warnings as errors, build, tests, smoke test, on the toolchain `rust-toolchain.toml` pins), never a direct push. Step 1 is what the `rust` check runs, steps 2 and 4 are the docs guard, step 3 is the release workflow's `gate` job; run them all locally anyway, the pipeline is the backstop, not the habit.
6. Rehearse the archive: build the seed, stage `bin/logos` with LICENSE, NOTICE, TRADEMARK.md and examples, pack it, unpack it, and run it. What ships must have been run.

Never tag speculatively "to see if CI passes". There is no undo.

# Branch rules (dev is the working branch; a branch lives only while its work is unmerged):
- An agent working on an issue does it in its own worktree, `.claude/worktrees/issue-N`, on a branch `issue-N-slug` from `dev`: one branch per issue, never one per slice, so agents never conflict with each other. Direct work on `dev` is for the session Thobias drives himself in the main checkout, and for edits outside the seed (CLAUDE.md, CLAUDE_LOG, memory).
- A branch stays local unless another machine or agent must read it. Pushing it to GitHub is not a backup and not a habit.
- Whoever merges a branch deletes it in the same command, local and on origin, and removes its worktree: `git merge --no-ff X && git branch -d X && git push origin --delete X && git worktree remove .claude/worktrees/issue-N`. A merged branch left behind is a bug, like an unrecorded ruling.
- Before creating a branch and at the end of every session: `git branch -r | grep -v 'origin/dev$\|origin/main$'` must list only branches with unmerged work, and the session log names each one and what it waits for.
- Reason: on 29 September 2026 GitHub held 43 branches, all but two merged into `dev` days or weeks earlier and never deleted (one per #137 slice, one per parallel agent). Thobias: "there are dozens of branches on github and i dont understand why. i dont like that."
