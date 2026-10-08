# Q-55: Can `a.type` be read before `a` is first written? And `a:dyad`: readable or refused?

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

## 1. Can `a.type` be read before `a` is first written?

Your #199 ruling moved the type read from the name (`a:type`) into the value (`a.type`): "type should always be accessed by .type instead of :type and the fact that you can access by . is defined in the body of type itself just like its defined how to access all the other fields". A `?` place has no value until its first write ("the declaration makes a binding, the first write makes the node"), and a read before that is refused. So this, which passed with `a:type`, is now refused:

```logos
mut a := i32 ?,
a.type == i32,    # refused: `a` is read before it is written
a = 9,
a.type == i32     # true
```

Your north-star demo does exactly that, inside the first write:

```logos
mut a := metatype(2) ?,
a = if (a.type == i32) (9)
    else if (a.type == f32 or a.type == f64) (9.9)
    else (i32),
print «{a}»       # 9.9 today, spelled a:type
```

- (a) **Refused.** No value yet, so no type to read: `.type` is a field like the others. The demo asks the computed type instead:
  ```logos
  t := metatype(2),
  mut a := t ?,
  a = if (t == i32) (9) else if (t == f32 or t == f64) (9.9) else (i32),
  ```
- (b) **Allowed.** The declaration already fixed the type, so `.type` of an unwritten place gives the declared type. The read-before-write check gets one exception, for the word `type`. A `dyad ?` place, whose type is not known before its write, stays refused. The demo stays as it is.

Recommended: (a), by the worker and by me. It is your #199 rule with no exception, and the demo still shows a computed type choosing the branch. Choose (b) if the demo's point is that a name's own type can be asked before it holds anything.

**Answer 1:** 

## 2. `a:dyad`: readable or refused?

Two rules disagree:
- ›`a.type` reads the type of the value…‹ (29 September, your #199): "The binding's field `dyad` points to the payload, so `a:dyad.type` is the long form", and its 27 September line: "`:dyad` and `:value` are not retired".
- ›A scope lays out its declarations; a use reaches the offset through its binding‹ (28 September, on the #168 plan), point (4): "`x:offset` and `x:frame` are readable … `x:value` and `x:dyad` stay refused".

The seed refuses `x:dyad` today.

```logos
x := i32 3, x:dyad.type    # (a): refused    (b): i32, the same as x.type
```

- (a) **Refused.** `a.type` and `a.value` are the only ways in; the 27 and 29 September sentences become history.
- (b) **Readable**, the long form of `a.type`. The 28 September "stay refused" becomes history, and making the seed read it is its own issue.

Recommended: (b), the newer text, which states the long form after 28 September. Nothing is built on either.

**Answer 2:** 

## Metadata
- **Status:** open
- **Priority:** nothing waits: #215 merged with (a), and (b) would add the exception and put the demo back; point 2 is one letter
- **Asked:** 2026-10-01 21:37
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#215](https://github.com/ThobiasKnudsen/LogosLang/issues/215), `a:type` becomes `a.type`
- **Branch and worktree:** none: #215 merged into dev at 0388214
- **Asked by:** logoslang-issue-215-worker (session b900f39e), Opus 5.5
- **Waiting:** nothing stops: #215 merged at 0388214 with (a), the demo rewritten in 5d365d7; (b) would add the exception for `type` and put the demo back. Nothing waits for point 2.
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [examples/metatypefn.logos](file:///home/o/Personal/Code/LogosLang/examples/metatypefn.logos): the station #30 north-star demo, shipped in the release archive
  - DESIGN.md: ›`a.type` reads the type of the value a name stands for (spelled `a:type` from 23 to 29 September 2026)‹, ›Reading a place before its first write is refused at parse‹
