# Q-17: a root that must wait for another root: does it keep the `root-cause` label?

## Metadata
- **Status:** answered, relayed 2026-09-29 23:00
- **Priority:** 4 (why: nobody waits on it; one letter answers it, and the answer fixes how issues are labelled from now on)
- **Asked:** 2026-09-29 22:19
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang); the rule is in the global [CLAUDE.md](file:///home/o/.claude/CLAUDE.md), so it holds in every repo
- **Issue:** [#203](https://github.com/ThobiasKnudsen/LogosLang/issues/203), what a type's `parse` writes into its node is placed three different ways; [#212](https://github.com/ThobiasKnudsen/LogosLang/issues/212), a type reading the value to its right is written three times; both wait for [#198](https://github.com/ThobiasKnudsen/LogosLang/issues/198), one check for a value landing in a typed place
- **Branch and worktree:** none
- **Asked by:** raised by logoslang-rca-alloc-param-deref-review-1 (session 8953575b, ended), Opus 5.5; written by the orchestrator
- **Waiting:** nobody is idle; #212 is filed the way (b) says and changes if you answer (a)
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the #197 question](../history/Q-15_issue-197.md) and [the #210 question](../history/Q-16_issue-210.md): unrelated, and both matter more

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. Label, or `Blocked by`?

Two analyses read the Issue rules two ways. Both found a structural fact of their own that can only be fixed after #198, because the fix needs #198's landing check:

```
#203  [root-cause]  Root-cause, blocked by nothing (examined 29 September 2026). … this one after #198 …
#212  [no label]    Blocked by #198: what this issue's one read takes lands through #198's `land` …
```

The Issue rules support both readings:
- "A `root-cause` issue is blocked by nothing; if it turns out to wait for another issue, that one is the root and the label moves."
- "Roots first, in dependency order, each alone while they share files."

- (a) **The label says what the issue is; the first line says when it can start.** A root that waits for another root keeps the label, and its first line reads `Root-cause, after #198 (examined …)`. "Blocked by nothing" in the rule becomes "caused by no other issue". The label moves only when the issue turns out to be a symptom of another.
- (b) **The label means "a root you can start now".** A root that waits loses the label, says `Blocked by #198`, and gets the label back when #198 lands.

Recommended: (a), because the order already lives in the first line, and with (b) the label has to move every time the order changes.

**Answer 1:** 
root causes can actually be blocked by other issues still. so you should change the prompts to reflect that orchestator. 
