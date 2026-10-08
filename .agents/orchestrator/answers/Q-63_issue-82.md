# Q-63: Q-59 round 3: what converts by itself, and three edges of what an `if` gives

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Your Q-61 answers are in DESIGN on dev as 45002b6:
- `return` and the statement words as nodes of their own;
- the `if` node's fields;
- the partial parse of a line that uses a run-time type;
- a new rule, ›A type with a `run` whose field is `?` mints too, so its `run` body is parsed once per mint‹.

Four edges are left, written as Open lines there. Each point stands alone.

**"is this already solved though?"** (how a node's output reaches whatever consumes it) Yes. A node never pushes its value: whatever consumes it runs it and takes what it gives (›Execution is function application: to evaluate a dyad, read its type; run a `fn`, run a type's `run`, else it is data‹). In `y := (a + 1) * 2`, the `*` runs the `+` and gets its value; a scope gives its last line (›A scope's value is what it evaluates to, and `return` is an optional early exit from the enclosing function‹). `return` differs only in who takes its value: whatever consumes the call, not its own line.

**"their types must always be the same … right?"** For two numbers, yes, after the parse's conversion step. But ›A pointer steps by whole cells‹ (your 23 September rule) adds an `@T` and an integer: `p + 1` gives the next cell. So I wrote "one mint per set of field types the parse chooses": `+` over two `i32`s is one mint, `+` over `@i32` and `i64` another. Say so if that is wrong.

## 1. Which values convert without their type written?

Your `if` rule ("convertable to the same type which is required outside the if") and your `a + 0` conversion step both depend on it. Today only a plain number does, and ›No implicit coercion; a numeric type applied to a value is the conversion‹ says so:

```logos
mut x := f64 0.0,
x = 9,              # 9.0: a plain number is converted where it lands
x = f32 9,          # refused today: "these types do not match"
x = f64 (f32 9)     # 9.0: the conversion written
```

- (a) **Only a plain number, as today.** An `if`'s arms may then differ only by plain numbers: `x = if c (1) else (f64 2.5)` is fine, and `x = if c (i32 1) else (f64 2.5)` is refused, written `if c (f64 (i32 1)) else (f64 2.5)`. `i32 1 + i64 2` stays refused.
- (b) **Any value whose type lies inside the required type**, as your Q-20 said: "i32 is a subset of i64 so that conversion is allowed". The required type's `parse` decides, and a value never converts into a type that cannot hold it exactly. So `x = f32 9` gives `9.0`. `x = if c (i32 1) else (i64 2)` into an `i64` place is an `i64`, and `+`'s parse chooses `i64` for `i32 1 + i64 2`, converting the `i32` side. ›No implicit coercion‹ is renamed to say a type's `parse` decides what converts into it. This also settles that rule's open line on which way the subset check runs.

Recommended: (b). It is what your Q-61 answers describe, and refusing lossy conversions keeps the danger of unwritten ones away. Pick (a) if every conversion beyond a plain number should stay visible in the text.

This replaces point 1 of Q-62.

**Answer 1:** 
b

## 2. What does an `if` give where no type is required?

```logos
x := if c (i32 1) else (i64 2),   # x has no type of its own
```

- (a) **The `if`'s parse picks the one arm type every other arm converts into,** as `+`'s parse picks one of its operands' types: here `i64` under 1 (b). If there is none, it is refused where it is written.
- (b) **A `dyad`:** `x` holds whichever arm ran, with that arm's type, and the lines that use `x` are finished when the run gets there, as in your Q-61 example.

Recommended: (a). It is the same choice `+` makes, and it keeps `x` one type, so the lines after it parse in full beforehand.

**Answer 2:** 
the actual type the user wants should be written explisitly as a cast of what ever if returns. the lazy parsing only happens when dyad is used explisitly. but to be clear this code example should not be alloved because it is ambiguous what type x should be as it can only be on type at parse time. 
write like this instead:
x := i64 if c (i32 1) else (i64 2),
if you want any type you write dyad instead of i64 in front of if. 

## 3. `y := if false (5)`: an `if` without `else` used as a value

- (a) **The error of binding nothing:** when the condition is false no arm runs, so there is no value. It is the same `if` type with or without `else` (your Q-59 words); only the value differs.
- (b) **`y` is `undefined`** when no arm ran, and reading it is the checked error.

Recommended: (a). The mistake shows where it is written, not later where `y` is read.

**Answer 3:** 
else is mandatory when using if as assigment. that should be showns as error message.

## 4. An arm that leaves the function

```logos
f := fn (c := i32 ?) -> f64 (
  x := if c == 1 (return f64 0.5) else (f64 2.5),
  x + 1.0
),
f(2)     # today 5e-324, should be 3.5
```

Your Q-61 answer: `return` hands its value to whatever consumes the call, so the `if` gets nothing from that arm.

- (a) **Only the arms that reach the `if`'s end must convert to the required type.** `x` is an `f64`. The same goes for an arm ending in `error.X «…»` or `abort «…»`.
- (b) **Refused,** written instead as `if c == 1 (return f64 0.5), x := f64 2.5`.

Recommended: (a). An early exit in one arm is the common shape, and it needs no special type.

**Answer 4:** 
not allowed: each if-else branch in an assignment must return something to the assignment operation. 

## Metadata
- **Status:** answered, relayed 2026-10-02 19:19
- **Priority:** #82's worker can finish the slice's type rules only with these; each is one letter
- **Asked:** 2026-10-02 15:28
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** Orchestrator (2), from your Q-61 answers (Q/A on #82, comment 5953319534)
- **Waiting:** #82's worker, which re-plans the slice under 45002b6 meanwhile
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-61_issue-82.md](../answers/Q-61_issue-82.md): your answers this round builds on
  - [63_Q-62_issue-82.md](../questions/35-Q62-I82.md): its `=` point moved here as point 1
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›`if` reads its own right side‹ and ›A type with a `run` whose field is `?` mints too, so its `run` body is parsed once per mint‹, whose Open lines these are
