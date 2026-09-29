# LogosLang's own rules

The general rules live in the global `~/.claude/CLAUDE.md` since 29 September 2026: Musk's rules, the behaviour rules, the faithfulness protocol with DESIGN.md as the ruling document, the issue rules (root causes first), the comment rules, the branch rules, and how files and commands are handled. They bind every agent in this repo. This file holds only what is LogosLang's own. Reason (Thobias, 29 September 2026): the general rules should reach every agent on the machine, in every repo. A reader who does not have that file (another machine, a contributor) asks Thobias for it.

# Behaviour
- The example Thobias gets is a Logos example wherever one fits.

# Issues in this repo
- The labels are `root-cause`, `question` and `in-progress`. The first-line forms are `Blocked by #N`, `Leaf, blocked by nothing (examined <date>)` and `Umbrella (examined <date>)`. Every open issue was linked this way on 29 September 2026.
- A repro is the Logos program, its exact output, the commit and the tier (interpreted or compiled).
- A root is tested along these axes: the named value against the unnamed one, the interpreter against the compiled tier, one type against another.
- The structural facts that are roots here: one rule written twice, once for the interpreter and once for the compiler; a Rust `match`/`if` on node kind where DESIGN says the record decides; a hand-kept list of kinds; a mechanism DESIGN has superseded; a rule DESIGN does not have yet.

# Faithfulness in this repo
- DESIGN.md is one rule per `###` heading (since 27 September 2026). Point at a rule by its heading, `«### like this»`, never by line number; a `DESIGN.md l.N` in a Source bullet, an issue or a memory means line N of the paragraph form at git e75bcdc.
- A ruling is recorded at the rule it touches: a **Ruled** line with date, who and reason; a **Rejected, to stay rejected** line; or a new `###` rule. What it supersedes is a **History** line in DESIGN_HISTORY.md under the same heading.
- `language_sketch.logos` illustrates DESIGN.md; a conflict between the two is surfaced as a blocking question like any other.
- Before starting work in a spec area, run /faithfulness-audit.

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

# Branches in this repo
- `dev` is the working branch and the base of every issue branch; `main` is protected and takes a PR (Release rules, step 5).
- `git branch -r | grep -v 'origin/dev$\|origin/main$'` must list only branches with unmerged work.
- The orchestrator reads this repo's settings from `.claude/orchestrate.conf` and `.claude/orchestrate-notes.md`.
