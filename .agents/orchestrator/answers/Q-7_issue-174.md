# Q-7: what should the slot be called, now that it gives back text instead of printing?

## Metadata
- **Status:** answered, relayed 2026-09-29 20:36
- **Priority:** 1 (why: the #174 worker is idle until you answer; one word answers it)
- **Asked:** 2026-09-29 20:40
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#174 what does a record print?](https://github.com/ThobiasKnudsen/LogosLang/issues/174)
- **Branch and worktree:** `issue-174-record-print`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174)
- **Asked by:** logoslang-issue-174-worker (session 5d14174f), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your four answers](../answers/Q-5_issue-174.md): "it gives back text so maybe there is a better name tjen print then?"
  - [DESIGN.md on the branch](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174/DESIGN.md): all four answers are recorded at ›A value is shown as the text its type's `print` slot gives back‹; only the name is open

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. The slot's name
Each candidate fills `pt`'s slot:

- (a) **`show`**: `share show = ( «pt({a}, {b})» )`. A plain verb like `parse`, `run`, `free`; unused anywhere in DESIGN.md.
- (b) **`text`**: says what comes back, but `.text` is already the string type's own field, so `s.text` would mean two things.
- (c) **`display`**: the word Rust uses. Clear, longer.
- (d) **`spell`**: close to "spelling", which already means a name's source text, a different thing.
- (e) **keep `print`**: one word for slot and output, as `free` is; but the slot does not print.

Recommended: (a) `show`, because slots are named for the moment they run (`parse` when the spelling appears, `run` when evaluated, `free` when life ends), this one runs when a value is shown, and it does not clash with `print «…»`.

What "gives back text" means today, for your information: the slot on every type, the core types' own fills (numbers, bools, rationals, type names, `void` showing nothing) and your default error can be built now. A fill written in Logos, even `share show = ( «a point» )`, waits for text the program can hold at run time, which the seed does not have. Until then every record printed is the error "pt fills no show", so the REPL cannot show a record yet.

**Answer 1:** 
print because i guess print could also mean print text in a string and return the string
