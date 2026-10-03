# Q-40: a tape spot marked "built" that holds no node: where is it refused?

## Metadata
- **Status:** withdrawn unanswered 2026-10-01 01:48, not needed now (Thobias asked to remove the questions not needed): it was asked so the analysis could file, and PWS replaces that filing. The finding is problem P31 in PWS.json; these points come back in the solution talk for P31's root.
- **Priority:** 1 (why: about twenty programs segfault this way; the analyst is idle and its filing waits for these three; one letter each)
- **Asked:** 2026-09-30 02:30
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** none yet: the analysis `constructed-null-cell` files a new root after your answer. Found by the #225 worker
- **Branch and worktree:** none, [analysis worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-constructed-null-cell)
- **Asked by:** logoslang-rca-constructed-null-cell-worker (session cf0f45fd), Opus 5.5
- **Waiting:** the analyst is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›The tape is Logic Graph with a string extension‹, ›`is_constructed` is the tape's own list, read `tape.is_constructed[k]`‹, ›Two levels: the binding is the lex level, the dyad is the value level‹, ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹
  - [RCA.md](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/rca-constructed-null-cell/RCA.md): §3 the root, §4 the 36 probes, §8 these questions
  - #127 and #202 (a `fn` parameter with no type) and #208 (a bracket in a `run` body) crash the same way from another source, and stay their own issues

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. -->

An unknown word's spot on the tape holds no node, only its text. Nothing stops a `parse` from marking that spot "built", and then about twenty programs crash the whole process (exit 139): other spots, function bodies, compiled code, imported files. All of them were plain errors before 29 September (#166 slice 1). DESIGN says "A constructed cell is already a graph dyad", but not what happens when the flag is written on a spot with no node.

## 1. A `parse` marks an unknown word's spot as built. What happens?

```logos
h := type (share parse_rank = *.parse_rank + 1, share parse = ( tape.is_constructed[1] = true, tape.remove(0) )), h zz, 5
# today: segfault
```

- (a) **The flag write is the error, where it is written:** "tape spot 1 holds no node, only the unknown word `zz`". The tape keeps "a built spot is a node", so the six hand-written "is it empty?" checks go.
- (b) **The write is allowed but does nothing there:** the spot stays the unknown word, and the line ends with the usual "unknown name `zz`". But `tape.is_constructed[1]` then reads `true` while the driver treats the spot as unbuilt: one bit, two meanings.
- (c) **The write is allowed, and a later read is the error.** Every reader must then check, which is exactly how the ten crashing places came about.

Recommended: (a). One program changes: `x h2 h1 zz`, where `h1` marks `zz` and `h2` removes it before anything reads it, prints `7` today and would be refused at `h1`'s write.

**Answer 1:** 

## 2. A `parse` reads such a spot: `tape[1]` or `tape[1]:type`. What does it get?

```logos
h := type (share parse_rank = *.parse_rank + 1, share parse = ( if tape[1]:type == void error «void», tape.remove(1), tape.remove(0) )), h zz, 5
# today: segfault at `tape[1]:type`; `tape[1]` hands back an empty pointer
```

`tape[1]:name` already answers "this cell holds no name", and DESIGN says "A read that needs more than a cell has (`tape[k].dyads`, `tape[k]:name`) is the error it is on any cell without one". The word's text is read with `tape.spelling[1]`.

- (a) **The error, like `:name`:** "tape spot 1 holds no node, only the unknown word `zz`; read its text with tape.spelling[1]". One answer for every read of that spot.
- (b) **`void`, as past the end of the tape.** But `void` means "nothing more here", while a word is here, and `tape[0] = tape[1]` would drop the word without a sound.
- (c) **A new answer meaning "unknown word".** A new identity DESIGN does not have.

Recommended: (a).

**Answer 2:** 

## 3. May a `parse` mark a spot other than its own, or write `false`?

DESIGN: "The cell's own constructor writes it `true` when it has built something", and the one exception to "never assigned" is "a constructor writes its own cell's flag, `tape.is_constructed[0] = true`". The seed takes any spot, `true` or `false`, so this runs:

```logos
x := 5, h := type (share parse_rank = *.parse_rank + 1, share parse = ( tape.is_constructed[1] = true, tape.remove(0) )), h x
# prints 5
```

Every Logos file and test in the repo writes only `tape.is_constructed[0] = true`.

- (a) **Keep DESIGN:** only `tape.is_constructed[0] = true`, on the constructor's own spot; any other spot, or `false`, is the error. The flag keeps one meaning: "this constructor finished". After `tape.remove(0)`, spot 0 is the next word, so that write is refused too.
- (b) **Widen DESIGN to what the seed does:** any spot, `true` or `false`, with a reason (say, a macro that marks a neighbour built). Question 1 then decides what happens on a spot with no node.

Recommended: (a). Nothing in the repo uses more.

**Answer 3:** 
