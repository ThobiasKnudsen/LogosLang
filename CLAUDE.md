# Follow Elon Musks rules faithfully:
1. **Question every requirement:** Find out who exactly created the requirement and never just accept that it's from "the safety or legal department". Make the requirements less dumb.
2. **Delete parts or processes:** Delete as many parts or steps as you can. Musk's rule of thumb: If you aren't forced to put back at least 10% of what you deleted, you didn't delete enough.
3. **Simplify and optimize:** Only do this after you have completed steps 1 and 2, because a massive waste of time is optimizing something that shouldn't exist in the first place.
4. **Accelerate cycle time:** Speed up the process.
5. **Automate:** Only automate the process after the first four steps are done.

# Faithfulness protocol (spec-governed code):
- DESIGN.md is the ruling document. language_sketch.logos illustrates it; issues, plans, memories, old comments, and existing code are downstream and may be stale. Never implement from a downstream source alone.
- Before implementing anything spec-governed, quote the exact DESIGN.md passage(s) that license it, in the plan or the commit message. No quote → stop and ask.
- If any two sources disagree — DESIGN vs sketch, DESIGN vs an issue, one DESIGN section vs another — STOP and surface the conflict as a blocking question, with both quotes. Never silently pick a side, even if one side is newer or was written by Thobias: staleness is invisible from inside a session.
- When a conflict is ruled on, or a design is rejected in conversation, record it in the same session: in DESIGN.md (the existing pattern: "Recorded as rejected, to stay rejected: …") or, if spec wording must wait, as an explicit pending-spec-edit in the session log AND auto-memory. An unrecorded decision is a future bug.
- Before starting work in a spec area not touched recently, run /faithfulness-audit.

# Release rules (a version tag is one-way):

**Release tags are immutable. Pushing one is irreversible, so a tag that fails CI burns that version number forever.**

The `freeze-release-tags` ruleset blocks both `deletion` and `update` on every `refs/tags/v*`. A pushed tag cannot be moved or removed by anyone short of editing the ruleset in repo settings. This is deliberate: a released version must always mean one commit. The consequence is that `git push origin vX.Y.Z` is a one-way door — if the release workflow then fails, the tag stays pinned to the broken commit, the version is spent, and the next attempt must use a **new, strictly higher** version. (This is exactly how v0.0.3 was lost: the `gate` job died on a first-release-only bug, and v0.0.4 shipped the identical tree.)

A spent version also freezes its docs. `docs-check.sh validate` treats any `docs/vX.Y.Z/` at or below the newest tag as frozen and fails the PR on any add, modify, or delete inside it — so the abandoned version's docs folder can no longer be removed either, and its replacement must be a copy under the new version.

Before pushing any version tag:

1. `cargo test --release` is green.
2. `bash .github/scripts/docs-check.sh validate` passes.
3. `bash .github/scripts/docs-check.sh release vX.Y.Z` passes — this is the exact check the `gate` job runs, and the in-progress `docs/vX.Y.Z/` folder must be named for the version being released.
4. `bash .github/scripts/docs-check.test.sh` passes (the guard's own self-test).
5. The tag is on `main`, and `main` is already merged and green. `main` is protected: it takes a PR with 2 required checks, never a direct push.
6. Rehearse the archive: build the seed, stage `bin/logos` with LICENSE, NOTICE, TRADEMARK.md and examples, pack it, unpack it, and run it. What ships must have been run.

Never tag speculatively "to see if CI passes". There is no undo.

# Follow logging rules faithfully:
- At session start create a file under CLAUDE_LOG folder with the name Session_YYYY-MM-DD_HH:mm:ss.md. 
- What should be said in the start of the file is the session id so that Claude can find the actual session later for more details. Format like this «# Session id: [session id]»
- At the end of each response for the user request you always append to the file, starting with a heading on this format «## response time: YYYY-MM-DD_HH:mm:ss | LLM: [LLM model responding] | user: [user name]». So you would need to ask for the user name if you don't know what it is. You can set "unknown" for that response but also ask for name so that you can set the name later.
- Under the heading you must write a summary of everything important in the last request and response.
- You can when ever you want search through previous logs
