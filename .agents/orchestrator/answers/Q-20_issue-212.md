# Q-20: `i32 m` with a name: three points on how a type reads the value to its right

## Metadata
- **Status:** answered, relayed 2026-09-30 00:19
- **Priority:** 6 (why: nobody waits; #212 cannot start before #198, and none of the three blocks its first slice)
- **Asked:** 2026-09-29 22:37
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#212](https://github.com/ThobiasKnudsen/LogosLang/issues/212), a type followed by a name is never one value in the seed (`m := i32 3, i32 m` is refused), because three copies of "a type reads its right side" disagree. It waits for [#198](https://github.com/ThobiasKnudsen/LogosLang/issues/198), one check for a value landing in a typed place.
- **Branch and worktree:** none yet
- **Asked by:** filed on #212 by logoslang-rca-alloc-param-deref-review-2 (session f63d0b50, ended), Opus 5.5
- **Waiting:** nobody
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [the analysis](file:///home/o/.claude/orchestrate/rca/logoslang/alloc-param-deref.md): every probe with its output
  - the other open questions come first

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. "all recommended" also works. -->

DESIGN says a type takes the value to its right into a new value of its type (`i32 3`), by one rule everywhere. Once #212 is fixed, `m := i32 3, i32 m` gives `3`. These three points are what DESIGN does not say yet.

## 1. A value of another type

```logos
m := i32 3, i64 m
```

- (a) **Refused:** "these types do not match". The conversion is written with brackets, `i64(m)`, as today, where `i32 2.5` is refused and `i32(2.5)` gives `2`.
- (b) **Taken as the conversion:** `i64 m` is `i64(m)`.

Recommended: (a), because then brackets stay the one way to write a conversion.

**Answer 1:** 
b obviously. i32 is a subset of i64 so that conversion is allowed

## 2. Record and pointer types

```logos
pt := type ( a := i32 ?, b := i32 ? ), p := pt(1, 2), q := pt p
m := i32 5, p := &m, q := @i32 p
```

Both are refused today. DESIGN's examples are all number types.

- (a) **Yes:** every type reads a value to its right by the same rule, so `pt p` and `@i32 p` work.
- (b) **No:** only number types read a value without brackets.

Recommended: (a), because DESIGN calls it "one rule everywhere".

**Answer 2:** 
this is defined in parse of pt, so this is actually not allowed since pt.parse doesnt really define anything here. it could be defined to take in tape[1] and check if itself is a subset of the type of RHS (tape[1]) and if so it allows the conversion and defines how it should be converted. in this case its the same type so its straight forward. this will also be explisitly defined in the parse for primitive types like f32 and u8. 

## 3. How far the value to the right reaches

```logos
i32 p.a      # i32 (p.a), or (i32 p).a ?
i32 q@       # i32 (q@)
i32 g(3)     # i32 (g(3))
i32 -m       # i32 (-m)
```

- (a) **The value is finished first:** `i32 p.a` is `i32 (p.a)`, and the same for the others.
- (b) **The type takes the next word alone:** `i32 p.a` is `(i32 p).a`, which asks a number for a field it does not have.

Recommended: (a), because (b) gives a number a field read or a call it cannot have.

**Answer 3:** 
in all these cases . @ g and - should have higher precedence, but on the case for - its a bit harder has there is usually a difference of what - means based on if there is space to its left and right or not. maybe there should be a regex for "-"|"- "|" - " meaning on identity and " -" is another? i think that would solve the problem
