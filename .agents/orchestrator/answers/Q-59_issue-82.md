# Q-59: What does a `return`, an `if`, a loop or a `:=` give back? (five points for #82's slice)

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

The slice makes every built-in node carry the type it gives back, as your `^` in `power.logos` already does: `==` gives `bool`, `+` its operands' type, a call its `-> T`. DESIGN says what most nodes give back. For these five it does not. Programs are on dev 0388214, interpreted.

## 1. What does `return x` give back to the line it stands in?

`return x` leaves the function with `x`. But it stands inside a line too, and the line needs a type for it:

```logos
f := fn (c := i32 ?) -> f64 (
  x := if c == 1 (return f64 0.5) else (f64 2.5),
  x + 1.0
),
f(2)     # today 5e-324, should be 3.5
```

Today a `return` counts as an `i32`, so this `if` is read as an `i32`.

- (a) **A type that fits anywhere**, because nothing ever comes out of a `return`. In an `if` the other arm decides; `(return x) + 1.0` is allowed and the `+` never runs. It needs a new type and a word for it; proposed: `never`, so `(return x).type` gives `never`. (Rust spells it `!`, which Logos already uses for errors, `T!`.)
- (b) **`x`'s type.** `if c (return f64 0.5) else (f64 2.5)` is an `f64`, but `if c (return f64 0.5) else (i32 2)` is refused.
- (c) **Nothing (`void`)**, and `if` gets an exception: an arm that is a `return` takes the other arm's type. `(return x) + 1.0` is refused.

The same answer covers `error.X «…»` and `abort «…»`, which never give back to their line either: `if c == 1 (error.bad «no») else (f64 2.5)` is an `f64` under (a), refused under (b), and needs the same exception under (c).

Recommended: (a). One rule and no exception in `if`. It fits DESIGN's "A value that is only a block's result is the block's last expression, never a `return`". Your Q-45 line `x := (if (c == 1) ( return 5 ) else ( i32 2 )) + ( free a, i32 1 )` works with (a) whatever the function returns; with (b) only when it returns an `i32`.

**Answer 1:** 
return should likely be its own node in the LG. and return holds one dyad which could be anything. it could be just a rational_number 5.5 or f64 43 or "if" or "type" or "fn (…) -> (…)". you see what i mean? so definition of return should have one field of type dyad. does this actually work? i dont know if im missing something. but my intuition is that as long as return is its own node in the LG it will work since if you would want to replace return with what it actually returns in the LG then you loose information because return itself exists the function. but reaturn has not "output_type" field in itself. because if it had then you could do something like this: «a := return x» which is not allowed. or actually this would work but a would become the return, not some numberic value like it is now. so you could actually write this but when calling a later it is «return x» so a and return x are synonymous. 

## 2. An `if` whose two arms give different types

```logos
f := fn (c := i32 ?) -> void (
  x := if c == 1 (i32 1) else (f64 2.5),
  print «{x}»
),
f(2)     # today prints 0: the 2.5 is read as an i32
```

- (a) **Refused where it is written**, as `i32 1 + f64 2.5` is ("these types do not match"). A plain number in one arm is converted to the other arm's type: `if c (1) else (f64 2.5)` is an `f64`. Two plain numbers give a `rational_number`, as `1 + 2` does. A program that wants either type says so with a dyad: `mut x := dyad ?, if c == 1 (x = i32 1) else (x = f64 2.5)`.
- (b) **The `if` gives back a dyad:** `x` holds whichever arm ran, with that arm's type, as a bare parameter does, and each use of `x` is checked when the program gets there (›A `dyad ?` place is transparent: a placeholder for a new node of any type‹).

Either way, when the condition is known at parse the untaken arm is skipped unread, so there is nothing to compare. Your `metatypefn.logos` demo (arms `9`, `9.9` and `i32`) is untouched. The worker's other option, turning the second arm into the first arm's type, is left out: ›No implicit coercion…‹ rules it out.

Recommended: (a). It is the same rule as for `+`'s two sides, `compile` keeps one type per node, and the dynamic case is still one line away.

**Answer 2:** 
this should only work if each branch gives of the same type which means all branches could give of dyad as type but that means we need parsing after runtime since x could get differing types in runtime which means that where it appears later needs to be parsed after the runtime has given the type. 

## 3. What do an `if` without `else`, a `while` and a `for` give back?

```logos
y := if false (5), y        # today 0
y := while false (1), y     # today 0
```

- (a) **Nothing (`void`)**, as a `-> void` call. DESIGN already says a loop body's value "is thrown away", and an `if` without `else` may not run its body, so it has nothing sure to give. `y := if false (5)` then binds nothing, as `y := f()` over a `-> void` `f` does, which #184 makes an error.
- (b) **An `if` without `else` gives back its body's type**. When the body did not run, `y` is `undefined` and reading it is the checked error. Loops give back nothing.

Recommended: (a).

**Answer 3:** 
they have no output_type field like return, but they arent just void nodes. they are if nodes while nodes and for nodes with their own fields. and if without else is the exact same type "if" as an if with else. the "if" node records all else if branches and optional last else branch inside its fields. same pattern for while and for but in a different way though. they also have their own type definition with their own fields just like return and if. else and else if doesnt have their persistant independent nodes. they get consumed by the first if they must come after. btw doesnt this allow for merging match and if into the same structure or how does match actually differ from if?

## 4. What do `:=` and the other statement words give back?

That is `x := …`, `defer …`, `free x`, `print «…»` and `compile f`.

```logos
y := (x := i32 5), y      # today prints nothing at all
```

- (a) **Nothing**, as `=` ("An assignment is an act, not a value") and `import` ("an `import` line has no value") already give. `y := (x := i32 5)` is then the error of binding nothing.
- (b) **`:=` gives back the value it bound**, as `=` does in C; the others give back nothing. `y` is `5`.

Recommended: (a). It is what DESIGN already says of `=` and `import`. The REPL echo and a function's last line can then drop their own lists of statement words: a line that gives back nothing prints nothing.

**Answer 4:** 
same as if while for and return. they are their own type nodes which record their own fields. and has no output_type field

## 5. Two rules disagree on the words of a `+` node

- ›Operands sit inline after the type word; a growing list stays behind a pointer‹: "A binary `+` is one block, `[+, LHS, RHS, type]` … The third word is filled once the operand type resolves, so run, compile and reflection dispatch on the stored concrete operation; a lowering rule fills it, never the operator's constructor at parse".
- ›A node's output type is per node, and its parse writes it‹: "The author declares `output_type := type ?`; the parse writes `tape[0].output_type = tape[-1].type`". Your `^` has that word.

The first rule lists no word for the type the node gives back. For `+`, the operation and its result share a type (`i32 +` gives an `i32`). For `<` they do not (`i32 <` gives a `bool`).

- (a) **Every node gets an `output_type` word**, written by its parse, as `^`'s node has: `[+, LHS, RHS, operation, output_type]`. The first rule's example gains the word. What it rules (operands inline, growing lists behind a pointer) stays.
- (b) **No new word:** the type is read from the operation word. But a lowering rule fills that word after the parse, while the parse needs the type at once ("a body using `^` gets its types in the one pass, no lookahead"). And an `if`, a `( … )` or a `return` has no operation word, so those would need a word anyway: two mechanisms.

Recommended: (a). The worker builds (a) meanwhile.

**Answer 5:** 
i havent said what im going to say now and will maybe become a rule: array is doing minting and so should + - * / ^ and also type. probably other things as well. and the reason is that each mint needs to have its own unique run and also for each mint the output_type could be shared instead of per instance. it seems like minting should be use many places but i dont exactly know the rule for when to use minting and when to not. its not needed for "if" "return" "while" etc, but is needed for array, +, -, ^ and type becasue it allows for comparison of minted types instead of all instances being unique and harder to compare. things like := and = im not sure if needs to be minted, because with those there is needed some way to express per type what := and = is actually writing, i think. there would also need to be a way to define how types convert between each other so e.g. how f64(f32 9) gets converted from f32 to f64. this is needed in = and := when neding to convert the type of RHS to the type of what is being assigned. but to answer concretely, + needs to be minted same as all other operations. 

## Metadata
- **Status:** answered, relayed 2026-10-02 14:18
- **Priority:** #82 is the root every other fix waits for, and its worker needs points 1 to 4 to finish
- **Asked:** 2026-10-01 22:48
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** logoslang-issue-82-worker (session 3b044388); examples rerun and two corrected by Orchestrator (2)
- **Waiting:** the worker goes on building the mechanism. It needs 1 to 4 to write what these nodes give back, and builds (a) of point 5 meanwhile. Each point stands alone.
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): ›A node's output type is per node, and its parse writes it‹, ›A scope's value is what it evaluates to, and `return` is an optional early exit from the enclosing function‹, ›No implicit coercion; a numeric type applied to a value is the conversion‹, ›`=` sits beside `:=`, and returns nothing‹
  - #184 (`y := f()` over a `-> void` `f` gives `0`) and #187 (a loop as the last line echoes `0`) wait on this slice.
