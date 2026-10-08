# Q-44: `share` lines after your Q-38 answers: made once at the definition, per call, or per set of types? And what "( t )" meant

## Metadata
- **Status:** answered, relayed 2026-10-01 02:32
- **Priority:** 0 (why: #197's third review round is done except for these; the reviewer is idle and #197 merges after them; 1 and 2 take a letter each)
- **Asked:** 2026-10-01 00:42
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#197](https://github.com/ThobiasKnudsen/LogosLang/issues/197), when `immediate` runs and where a body is built
- **Branch and worktree:** `issue-197-build-where-written`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-197)
- **Asked by:** logoslang-issue-197-review-3 (session fef213ff), Opus 5.5
- **Waiting:** the reviewer is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [Q-38_issue-197.md](file:///home/o/.claude/orchestrate/history/Q-38_issue-197.md): Q-38, the answers these points come from
  - [Q-28_issue-197.md](file:///home/o/.claude/orchestrate/history/Q-28_issue-197.md): Q-28, where a `share` line in a body is made at the definition
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›Two muts, and the storage partition‹, ›The pass runs only as far as it must, in order, and never twice‹, ›`fn` is not a primitive: a function is a type in this same shape‹
  - [Round 3's comment on #197](https://github.com/ThobiasKnudsen/LogosLang/issues/197#issuecomment-5920953964): what the round recorded and fixed
  - [#202](https://github.com/ThobiasKnudsen/LogosLang/issues/202), the seed builds a generic `fn` body at its definition, not once per set of argument types; [#231](https://github.com/ThobiasKnudsen/LogosLang/issues/231), your idea that a `-> type ( … )` body is itself the type's scope

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Round 3 recorded both your Q-38 answers. Your reason for answer 2 was "the body of a function and type.run always runs per call of the function". It leaves three points open.

## 1. Is a `share` line in a function body still made once, at the definition?

Read alone, "the body runs per call" would run a `share` line at every call too. Your Q-28 answer 2 ("a", 30 September) makes it once, at the definition, and so does ›Two muts, and the storage partition‹: "in a function body: every call (C's static local)", "The initializer runs once, when the definition is parsed".

```logos
bump := fn () -> i32 ( print «bump», 1 ),
f := fn () -> i32 ( share k := bump(), k ),
print «defined», f(), f()
```

- (a) **Unchanged: once, at the definition.** `share` is the marked exception to "the body runs per call", so this prints `bump defined 1 1`. DESIGN says this, and the seed does it.
- (b) **Per call:** prints `defined bump 1 bump 1`. A `share` line in a function body then no longer keeps its value between calls.

Recommended: (a). Your Q-38 answer was about a `share` in a `type (…)` that the call builds, and `share` exists to keep a value across calls ("persistent storage across runs of the same function", your words, 25 September).

**Answer 1:** 
b per call because it creates a new type each time. the share is a different one for each new type. 

## 2. A body built once per set of types: one `share` place, or one per set?

This is a generic `fn` body, or a type's `run` body, which is built once for each set of operand types. The sentence the branch wrote in ›The pass runs only as far as it must, in order, and never twice‹, "made once when what holds it is built", can be read both ways.

```logos
bump := fn () -> i32 ( print «bump», 1 ),
sq5 := type ( a := ?, output_type := type ?, share run = ( share c := bump(), a * a ), share parse = ( …, tape[0].output_type = tape[-1].type, … ) ),
print «defined», x := i32 3, y := x sq5, z := f64 2.0, w := z sq5
```

- (a) **One place, made where it is written:** prints `bump defined`. This is your Q-28 answers taken together: a `share` line in a body is made at the definition, and a body is built in part where it is written. A `share mut n` in `^`'s `run` body would then count every call, over every type.
- (b) **One place per set of types, made when that set's body is built:** prints `defined bump bump`. This is your Q-38 answer 2 applied to a function read as a type (›`fn` is not a primitive: a function is a type in this same shape‹): each set's body is its own function, as each type `mk3` makes has its own `g`. C++ gives each template instance its own `static` the same way. The seed does (b) for a `run` body today.

Recommended: (a). It needs no new rule beyond Q-28, and the author wrote one function. The starting value cannot read the parameters (that is a checked error), so under (b) every set would compute the same starting value anyway. Its cost: the seed's `run` bodies change.

**Answer 2:** 
since sq5 is not inside a run body share c would be defined only once so option a. 

## 3. What did you mean by the last sentence of your Q-38 answer 2?

The sentence: "you could also have only ( t ) as the body of the function and it would be the same thing as the example you made here". It is recorded word for word, and nothing is read from it as a rule yet.

- (a) **Your #231 idea:** `mk3 := fn (t := type ?) -> type ( share e := t, share g := bump() )`, without the inner `type`, does the same as `mk3` (prints `defined bump bump`). Its `share` lines are then the type's, made once per type, not the function body's. That goes on #231, not into DESIGN now.
- (b) **Only an illustration:** a body `( t )` also runs at every call. Nothing more to record.
- (c) **Something else**, in your words.

No recommendation: only you know what you meant.

**Answer 3:** 
i meant that since its inside a run body which the body of a function is, the type would be constructed every time the function runs just like if the body was ( t ) or ( 1 + 2 ). if it wasnt inside a function and you called mk3 which is defined not as a function it will oly run at that place and not when mk3 is called

## 4. Answer 1 again: calling `f` makes no new type

Answers 2 and 3 are relayed to the reviewer. Answer 1 is held, because its reason does not fit its example. Your answer: "b per call because it creates a new type each time. the share is a different one for each new type." But in question 1's example no type is made:

```logos
bump := fn () -> i32 ( print «bump», 1 ),
f := fn () -> i32 ( share k := bump(), k ),
print «defined», f(), f()
```

`f` is one type, built once at its definition (›`fn` is not a primitive: a function is a type in this same shape‹: "exactly one specialization, built at definition"). Each call is an instance of it, not a new type (›Resolution is one rule‹: "A call frame is an instance of its function"). "A different `share` for each new type" is your Q-38 rule, and it stays for `mk3`, whose body builds a new `type (…)` when it is called with a new `t` (point 5).

Your answer 2 points the other way too: "since sq5 is not inside a run body share c would be defined only once". `f` is not inside a run body either.

DESIGN says a `share` line in a function body keeps its value across calls (›Two muts, and the storage partition‹): "in a function body: every call (C's `static` local). `share mut n := i32 0, n = n + 1` counts the calls", and "The initializer runs **once, when the definition is parsed**, not at the first call". Your words of 25 September: "it can be used on names inside functions to get some persistent storage across runs of the same function".

- (a) **Once, at the definition, for `f` as for `sq5`.** Prints `bump defined 1 1`. A `share` line is made again only where a call builds a new type, as in `mk3`. DESIGN stays as it is.
- (b) **Per call, in every function body.** Prints `defined bump 1 bump 1`. Then `share mut n := i32 0, n = n + 1` in a function no longer counts calls, a `share` line in a function body means the same as an unmarked line, and the DESIGN passage and your 25 September words are superseded. This needs a reason to record.

Recommended: (a). I read your answer 1 as your Q-38 rule for a type built at a call, and `f` builds none.

**Answer 4:** 
a

## 5. Answer 3: does a call with the same values still get the same type?

Your answer 3: "the type would be constructed every time the function runs just like if the body was ( t ) or ( 1 + 2 )". The ruling of 29 September at ›A `type (…)` inside a body is built once per set of the values it reads‹ builds that type once for each set of values it reads, and a later call with the same values gets the type already built:

```logos
mk := fn (t := type ?) -> type ( type ( share e := t ) ),
mk(i32) == mk(i32)    # true under the 29 September ruling
```

Your reason then: "there isnt often types are created so i would lean towards b since that allows for more general templates for types".

- (a) **Unchanged: once per set of values.** The `type (…)` runs at every call, like `( t )`, and a call with a `t` it has seen gets the same type back, so `mk(i32) == mk(i32)` is true. "Constructed every time" then means it runs every time, not that a new type is made every time.
- (b) **A new type at every call.** `mk(i32) == mk(i32)` is false, and the 29 September ruling is superseded. This is the option you set aside then, so it needs a reason to record.

Recommended: (a). Nothing in your answer asks to undo the 29 September ruling, and the reviewer reads it the same way.

**Answer 5:** 
b

## 6. Answer 5: did you mean the 29 September "b"?

Answer 4 waits to go to the reviewer with this one, in one message.

The letters swapped between the two questions. On 29 September your "b" was **once per set** of the values the type reads, with the reason "there isnt often types are created so i would lean towards b since that allows for more general templates for types" (›A `type (…)` inside a body is built once per set of the values it reads‹). Here, (b) is the other case, **a new type at every call**, the one that rule rejects: "the same values gave a new type at every run (`mk(i32) == mk(i32)` false)".

```logos
mk := fn (t := type ?) -> type ( type ( share e := t ) ),
mk(i32) == mk(i32)    # (a) true: once per set.  (b) false: a new type at every call
```

- (a) **Once per set, as on 29 September.** The letters were mixed up.
- (b) **A new type at every call.** A rule that is replaced records why, so give your reason in a line: what does a new type at every call let you do that once per set does not?

**Answer 6:** 
ok i see now, it would actually be false because there is nothing defined to do minting of anything a function runs. if you want minting you would have to somehow explisitly define that what things you are minting on exactly inside the function body
