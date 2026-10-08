# Q-18: four follow-ups to your seven `dyad` answers, and the answer to your question

## Metadata
- **Status:** answered, relayed 2026-09-29 22:55
- **Priority:** 2 (why: the #199 worker goes idle once it has recorded your first answers; each point has a recommendation, so "all recommended" answers it)
- **Asked:** 2026-09-29 22:30
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), a bare parameter `fn (a)` is `a := dyad ?`, a placeholder that takes any type. Point 2 also bears on [#209](https://github.com/ThobiasKnudsen/LogosLang/issues/209), where a type's `run` body picks its build by a list that knows only number types. Point 4 is [#201](https://github.com/ThobiasKnudsen/LogosLang/issues/201), how to compile a function whose parameter type varies per call.
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-worker (session 6dd47466), Opus 5.5
- **Waiting:** the worker records your settled answers now, then is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your seven answers](../answers/Q-14_issue-199.md): what these follow up
  - [the root-label question](../answers/Q-17_issue-203.md): unrelated

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. "all recommended" also works. -->

## Your question under answer 1

"the argument section in the definition of a function is lexed first then parsed per call site. is that correct?" Nearly: it is the body, not the argument section. The parameter list is read once, where the function is defined. When a parameter's type is left open (`a := ?`, and since your answer 3 a bare `a`), the body is kept as lexed text and built once for each set of argument types, the first time a call brings that set. A later call with the same types reuses that build. DESIGN ›A generic function body is lexed once and built once per set of field types‹: "The body is held as its lexed tape … and constructed once per field-type set when a node supplies the types". The seed does not do this for `fn` yet: it builds such a body only once, for the first call's types (#202).

```logos
double := fn (a) -> i64 ( a + a ),   # the parameter list is read here; the body is only lexed
double(i64 2),                        # builds the body for an i64, then runs it: 4
double(i64 5),                        # same type: reuses that build: 10
double(f64 2.5),                      # builds one for an f64: an f64 under -> i64, refused at this parse
```

## 1. What does `a` hold after `f(x + 1)`?

You picked B ("kept as graph") and said the call binds "the exact same way as a := x + 1". Those two may disagree, because `x := i64 5, a := x + 1, a:type == i64` is `true` today: `a` is the value 6, not the `+` node.

```logos
f := fn (a) -> bool ( a:type == i64 ),
x := i64 5,
f(x + 1),
```

- (a) **`true`:** `a` is bound exactly as `a := x + 1` binds it, to the value with its type. Nothing runs while parsing; `x + 1` runs when the call runs.
- (b) **`false`:** `a` holds the `+` node itself, and `a:type` is `+`.

The same choice decides this: in `f := fn (a) -> i64 ( a + a ), f(g())`, does `g` run once, as in `a := g(), a + a` (a), or once per read of `a` (b)?

Recommended: (a). It is what "the exact same way as a := x + 1" gives, and either way nothing runs while parsing unless you write `immediate`.

**Answer 1:** 
the things is that the actual LG and the binding per name is sort of separate and when something calls a field from the binding it needs to run the code so that it knows that field but the same LG still exists. so it should still return true because a:type does get the i64 type but if you do a:start you get to the actuall LG where a is declared and defined where you can inspect the nodes a, :=, x, + and 1. 

## 2. What does `:type` of an expression nobody named give?

Linked to point 1. The seed answers the node's own type word, not the type of what the expression yields. With `sq := fn (n := i32 ?) -> i32 ( n * n )`, `x := i32 3`, `n := i32 3`:

```logos
sq(5):type                 # 0
(x + i32 1):type           # type
(x < i32 7):type == bool   # false
(&n):type == @i32          # false
c := sq(5), c:type         # i32   (the same value, named)
```

DESIGN points both ways. ›`a:type` reads the type of the value a name stands for‹ lists "`tape[-1]:type` inside a constructor", and ›A node's output type is per node, and its parse writes it‹ writes `tape[0].output_type = tape[-1]:type`, so there `:type` must give what the operand yields. But ›`:` reads a name's binding, `.` reads a thing's own fields‹ says "in `b := a + 1`, `b:start.rhs.type` reads the `+` node's type", and ›`a:type` reads…‹ says "`:type` is the one `:` read that reaches the dyad level".

- (a) **The type of what it yields:** `sq(5):type` is `i32`, `(x + i32 1):type` is `i32`, `(x < i32 7):type` is `bool`, `(&n):type` is `@i32`. Named and unnamed agree.
- (b) **The node's own type:** `(x + i32 1):type` is `+`. A node's result type is read another way (its `output_type`).

Recommended: (a). The named and unnamed forms of one value then give one answer, and `tape[0].output_type = tape[-1]:type` keeps working.

**Answer 2:** 
good question and i think the answer is that since type lives in the same payload as the value now type should always be accessed by .type instead of :type and the fact that you can access by . is defined in the body of type itself just like its defined how to access all the other fields. the field in the binding says dyad and points to the payload which contains type and value. this is obviously a new ruling. 

## 3. `f(42)`: does a `rational_number` land in `-> i64`?

Settled by your answer 6: a plain number is a `rational_number`, so `a` holds a `rational_number` 42. What is open is the landing:

```logos
f := fn (a) -> i64 ( a ),
f(42),
```

- (a) **It gives `42`:** a `rational_number` commits where it lands, as ›Numeric literals are uncommitted until context classifies them‹ says ("stays `rational_number` until it lands in a typed slot"). `f(1/2)` is refused ("no exact value in the type it lands in").
- (b) **It is refused where the call is parsed:** a `rational_number` is its own type, and `-> i64 ( a )` needs `i64(a)` (›No implicit coercion‹).

Recommended: (a). DESIGN already lets a rational commit where it lands, and `f(42)` is what people will write.

**Answer 3:** 
the working of "stays rational_number until lands in a typed slot" is wrong wording because rational_number is already a type. its just that i64 converts rational_number to i64, which should be part of the definition of i64. f(1/2) is refused yes because 0.5 cannot be converted to i64. 

## 4. Compiling a function with a bare parameter (asked again, with the context)

The context is your answer on #201: "it can actually be compiled but it needs specific compilation for each callee since the type may vary. the solution may be to do some sort of minting per combination of types". Your answer 3 now gives that minting: each call with new argument types builds its own body of `f`, with fixed types, and each build compiles like any typed function. What is left is when a build made after `compile f` gets compiled:

```logos
f := fn (a) -> i64 ( a ),
f(i64 42),     # builds f for an i64
compile f,
f(i32 7),      # builds f for an i32, after the compile
```

- (a) **`compile f` compiles every build of `f` that exists, and each later build (`f(i32 7)`) is compiled when it is made.** The same would hold for `fn (a := ?)`.
- (b) **`compile f` compiles the builds that exist; a later build runs interpreted until `compile f` is written again.**

Recommended: (a). `compile f` then means "f runs compiled", whatever types it is called with, which is your "specific compilation for each callee".

**Answer 4:** 
i think actually b but there should be a field called auto_compile. there are actually some things that cannot be compiled at all im quite sure. for example when the function starts to reflect on itself recursively while it also changes itself. this is where mut of the boy comes in though, and is in most cases has more cons than pros but i imaging that some things could actually have use for this ability and in such case it cannot be compiled. btw since we are talking about minting here, functions should store mints of combinations of types just like array does it. btw compile compiles all the functions that is already minted if the user wants to batch compilations for performance, but can turn auto_compile = true for simplisity

---

For your information, no answer needed: the worker recorded answer 3 as covering every case where a dyad's held type is known only when the program runs. The program parses that use when it gets there, so a mismatch is still a parse error. ›A block settles its boxes as the top level does‹ keeps its checked error for a box used as an identity, such as a `type ?` in `x := t 5`. Say if a dyad should follow that rule instead.
