# Q-5: four points your `share print` ruling leaves open

## Metadata
- **Status:** answered, relayed 2026-09-29 20:32
- **Priority:** 1 (why: the #174 worker is idle until you answer; four short answers, each can be "yes" to the recommendation)
- **Asked:** 2026-09-29 20:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#174 what does a record print?](https://github.com/ThobiasKnudsen/LogosLang/issues/174)
- **Branch and worktree:** `issue-174-record-print`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174)
- **Asked by:** logoslang-issue-174-worker (session 5d14174f), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator
- **Related:**
  - [your first answer](../history/Q-4_issue-174.md): "there should be a share print function in each type to define how something should be printed"
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-174/DESIGN.md): the new rule ›A value prints the way its type's `share print` function says‹, recorded in your words with these four points marked Open

## Question

**1. Is `print` a slot every type has (like `free`), or a function each type writes itself?**
- (a) A slot: `pt := type (a := i32 ?, b := i32 ?, share print = ( print «pt({a}, {b})» ))`. One word does both jobs, as `free` already does: with `=` to its right it fills the slot, otherwise it prints.
- (b) Each type declares its own function: `share print := fn () ( … )`. Then the slot word and the output word `print «…»` clash inside a `run` body.
- Recommended: (a).

A slot every type has like free and parse

**2. Does the print function write the output itself, or give back text?**
- (a) Writes: `share print = ( print «pt({a}, {b})» )`. Works today; each `{a}` calls `a`'s own print.
- (b) Gives back text: `share print = ( «pt({a}, {b})» )`. Needs strings built at run time, which the seed does not have yet.
- Recommended: (a).

it gives back text so maybe there is a better name tjen print then?

**3. What prints when a type has no `print`?**
- (a) A default from `type`: the name and the fields, `pt(3, 4)`; nested `line(pt(0, 0), pt(3, 4))`.
- (b) An error: "pt has no print".
- (c) Nothing.
- Recommended: (a), because printing cannot harm anything and the REPL needs every value to show something.

an error. the default implementation of print is an error

**4. Confirm the worker's reading, yes or no:**
- #182 (`x:frame`, `x:scope`): these are pointers, so the pointer type's print decides what shows. #182 stays its own question.
yes
- #175, #184, #187 (a `-> void` call or a loop as the last line prints `0`): fixed by `void`'s print printing nothing.
yes

Not a question: the last line, `print «{p}»` and `error «{p}»` all use the same print, because DESIGN already says so.
yes that is ok for now. but when building the error system more precicely this willl maybe change

## Options
Answer per point, e.g. "1a 2a 3a 4 yes", or in your own words.

Recommended by the asker: 1a, 2a, 3a, 4 yes.

## Answer
<!-- Write below this line and save. Your words go to the agent verbatim. -->
