# Q-12: #174 review round 2 made four small DESIGN.md edits: merge now, or a third round?

## Metadata
- **Status:** answered, relayed 2026-09-29 21:11
- **Priority:** 2 (why: nothing runs on #174 while it waits, so nothing is spent; one word answers it)
- **Asked:** 2026-09-29 21:08
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#174 what a record prints](https://github.com/ThobiasKnudsen/LogosLang/issues/174), [round 2's report](https://github.com/ThobiasKnudsen/LogosLang/issues/174#issuecomment-5896724220)
- **Branch and worktree:** `issue-174-record-print`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174)
- **Asked by:** Orchestrator (2), Opus 5.5
- **Waiting:** nothing runs on #174; the branch stands with the gate green
- **Orchestrator:** Orchestrator (2)

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. Merge #174 now?

Round 2 found round 1's six fixes held, and made four edits, all in DESIGN.md, none changing a rule:

1. The print rule's open line now also names pointers other than `@dyad` as open (`x := i32 5, &x` prints an address today).
2. The two rules that gained `print` now say so in their Ruled lines, like the other slot lists.
3. ›Slots are named for the moment they run‹ said "Seed: done"; the `print` slot is not built yet, so it now says that.
4. One sentence said "the sentence above" where two sentences say it; it now names both.

The floor you adopted counts any DESIGN.md change as FIXED, so it would be one more round.

- (a) **Merge now.** And from now on, a DESIGN.md edit that changes no rule (a record line, a Seed line made true, an open point named, a pointer made precise) counts as cosmetic.
- (b) **Run round 3** on the four commits.

Recommended: (a), because none of the four changes what a rule says, and #166 went the same way.

**Answer 1:** 
merge now