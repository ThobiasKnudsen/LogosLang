# Q-62: Your question from Q-59: how does `match` differ from `if`?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Nothing waits for this. Its first point, whether `=` converts by itself, moved to Q-63 as point 1, because your Q-61 answers made it the one your `if` rule depends on.

## 1. Your question: how does `match` differ from `if`?

Both are a list of (test, body), with an optional last body. They differ in the test:
- `if` tests one `bool` per arm: `if x < 0 (…) else if x == 0 (…) else (…)`.
- `match` tests one value against patterns, and an arm can name what it finds, such as the error inside a `T!` (›Target: errors are values (`T!`), handled by `match`, passed on by `try`‹).

DESIGN has `match` only as that Target, with no form written yet. So one node shape could serve both: arms of (test, body) and an optional last body, where `match`'s tests are patterns on one value.

- (a) **Record it as a direction** at the `match` rule: "`match` and `if` share one node shape; a `match` arm's test is a pattern on one value and may name what it finds". It is decided when `match` is built.
- (b) **Leave it** until `match` is built.

Recommended: (a), so whoever builds `match` starts from your idea.

**Answer 1:** 

## Metadata
- **Status:** open
- **Priority:** nothing waits; a direction for when `match` is built
- **Asked:** 2026-10-02 14:17
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), where it came up (Q-59)
- **Branch and worktree:** none
- **Asked by:** Orchestrator (2), from your Q-59 answers
- **Waiting:** nothing
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-59_issue-82.md](../history/Q-59_issue-82.md): your answer 3, where the question comes from
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): the rule quoted above
