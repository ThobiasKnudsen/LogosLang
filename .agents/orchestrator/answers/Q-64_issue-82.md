# Q-64: #82: build the mints now, or merge the output-type read first?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

The #82 worker asks this. Its slice has one core: every place that needs a node's output type asks one read for it, instead of each place guessing. That read is built (commit 55441c8 on its branch). Your minting rule changes only where the read finds the type:

```logos
a := i32 5,
b := a + 0,
# today:      the `+` node carries its own word, "gives i32"
# with mints: the `+` node's type is "`+` for i32", and that mint holds "gives i32" once
```

Eight issues wait for #82 to merge: #184, #187, #175, #177, #174, #209, #194, #228.

- (a) **Build the mints in this slice.** There would be one mint maker for every built-in operation whose field is `?`, as `array` mints today. The run, the compiler, the display and about 16 checks of a node's type all go through the mint. This makes the slice about twice its size, and the eight issues wait that long.
- (b) **Merge the read first, then the mints as their own issue.** Until then, the built-in operators keep a word in each node, marked as a stand-in for that issue. The issue then changes the one read and the last line of each operator's builder, and nothing else. It shares the mint maker with #222 (data types that mint) and #202 (a `fn` built once per set of argument types). I would file it as a root, blocked by #82, and name it in DESIGN's Seed line under the minting rule.

Not offered: minting only `+ - * / %` by hand now. A hand-kept list of which operations mint is the kind of structure that has caused bugs in this repo before.

Recommended: (b). It follows your issue rule "Roots first, in dependency order, each alone while they share files": #82 and the mints are two roots sharing the same files. None of the eight issues depends on where the word is stored.

**Answer:** 
b

## Metadata
- **Status:** answered, relayed 2026-10-02 19:35
- **Priority:** decides whether #82 can merge before Q-63 is answered, and eight issues wait for that merge, so it sits just before Q-63
- **Asked:** 2026-10-02 15:51
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type` (dev merged in as 3e4b4ef), [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** the #82 worker (QUESTION #82, 2 October 15:43), through Orchestrator (2)
- **Waiting:** #82's worker, which builds the half that is already decided meanwhile (statement words give their line nothing)
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [05_Q-63_issue-82.md](../answers/Q-63_issue-82.md): the type rules the slice's `if` waits for
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›A type with a `run` whose field is `?` mints too, so its `run` body is parsed once per mint‹, whose Seed line this decides
