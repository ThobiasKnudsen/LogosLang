# Q-33: what may one REPL line hold, and what does a program with nothing in it do?

## Metadata
- **Status:** answered, relayed 2026-09-30 01:41
- **Priority:** 1 (why: the analyst is idle and its filing waits only for these two answers; one letter each, little reading)
- **Asked:** 2026-09-30 01:35
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `comment-only-segment` files a new root after your answer. Found by the review of [#220](https://github.com/ThobiasKnudsen/LogosLang/issues/220) (one reader for `«…»` and `#`)
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-comment-only-segment)
- **Asked by:** logoslang-rca-comment-only-segment-worker (session 670404dd), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›The command line is Logos source; the binary stays out of the way‹, ›The pass runs only as far as it must, in order, and never twice‹
  - [#225](https://github.com/ThobiasKnudsen/LogosLang/issues/225), a comment lifted at each build step: being built, a different root
  - [RCA.md](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-comment-only-segment/RCA.md), §8: the full analysis

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

`logos '# hi'` and the REPL line `# hi` fail with "nothing to evaluate here", while `( # hi )` and an imported file holding only `# hi` run. The cause: the seed reads a scope's lines in five places (a `( )` block, an imported file, the command line, a REPL line, a `{…}` inside a quote), and each ends its own way. DESIGN has one mechanism for all of them, so the fix makes it one reader. That needs two answers.

A program of only comments prints nothing whatever you answer: DESIGN already says a comment is void-valued.

## 1. What may one REPL line hold?

DESIGN says "the command line is a one-shot REPL line", and "The same text means the same in a block, at top level, in a REPL line and in an imported file". The seed's REPL has said "one expression per line" since 15 July, before those rules were written:

```
$ logos 'x := i32 1, y := i32 2, x + y'
3
» x := i32 1, y := i32 2
<repl>:1:11: error: one expression per line in the REPL
```

- (a) **A REPL line is a line of the program**, like the command line: any number of items separated by `,`, and the last one's value is echoed. `x := i32 1, 5` echoes `5`.
- (b) **One expression per REPL line**, while the command line takes many. The two DESIGN sentences get reworded.

Recommended: (a). It is what DESIGN's own words say, and the command line already does it.

**Answer 1:** 
a. btw im thinking of making it possible to define functions line by line in REPL without it failing so you could write this
logos
» f := fn () -> i32 (
»   2+3
» )
» f()
5

without any errrors. this is something that should be recorded in DESIGN.md and should also be an issue

## 2. What does a program with nothing in it do?

DESIGN has no passage on it, and the seed gives four answers:

```
$ logos ''        # error: nothing to evaluate here, exit 1 (also for '   ')
                  # an empty imported file runs; a blank REPL line is skipped; `()` is an empty scope
```

- (a) **No value:** it prints nothing, exit 0, the same as an empty file, a blank REPL line and `()`.
- (b) **The command line refuses** an argument of only whitespace, as a guard against running `logos ''` by mistake, and DESIGN records it as a command-line rule.

Recommended: (a). One reader then gives one answer everywhere.

**Answer 2:** 
a and you continue in repl