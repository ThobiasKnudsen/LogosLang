# Q-8: when does `immediate` run inside a body that is written once and built later?

## Metadata
- **Status:** answered, relayed 2026-09-29 20:46
- **Priority:** 2 (why: nothing is idle waiting on it; one letter answers it)
- **Asked:** 2026-09-29 20:45
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), its first comment holds the whole analysis
- **Branch and worktree:** none yet; a worker records your ruling in DESIGN.md after you answer
- **Asked by:** the root-cause analysis `immediate-in-run-body` and its review, Opus 5.5
- **Waiting:** no session waits
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the analysis](file:///home/o/.claude/orchestrate/rca/logoslang/immediate-in-run-body.md): every probe and quote
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`immediate x` runs the expression to its right as soon as it is parsed, and stands as its value‹

<!-- Write your answer on the **Answer** line, or anywhere else in the file, and save. -->

## 1. The moment `immediate` runs in a stored body

Today:

```logos
f := fn () -> i32 ( immediate ( print «hi», 1 ) )
# prints hi once, when the definition is read

sq2 := type ( a := i32 ?, share run = ( immediate ( print «hi», 1 ), a * a ) )
# prints nothing: it runs only when the first sq2 node is built,
# and again for each new set of field types (twice for i32 then f64)

for 0..3 ( t := type ( share g := immediate ( print «hi», 1 ) ), print «iter» )
# hi iter hi iter hi iter: a type written in a loop body runs it every iteration
```

DESIGN says `immediate` runs "as soon as it is parsed" and, inside a deferred body, "once, and the body holds the value". It never says which moment counts for a body that is stored where it is written and built later.

- (a) **Once, where it is written.** `sq2` above prints `hi` when its definition is read, like `f`. Only a per-type-set constant is lost.
- (b) **At each build**, what the seed does now. DESIGN gets that sentence; `sq2` prints only when a node is built.
- (c) **Split:** a non-generic body is read where it is written, like a `fn`; a generic one follows (a) or (b).

Recommended by the analysis: (a), because the rule's own reason is "the author asks for it in the source", and a stored body already resolves its names where it is written.

**Answer 1:** 
case 1 and 2 is correct since immediate executes right when its parsed, but the third example with the loop prit hi iter iter iter because its not parsed three times only once