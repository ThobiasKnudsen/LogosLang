# Q-61: Q-59 round 2: what a line sees where a `return`, `if` or `:=` stands, and minting as one rule

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save.
     Every save wakes the orchestrator; it relays once each question has an answer. -->

Your Q-59 answers say what each node *holds*: its own type, its own fields, no `output_type`. The worker also needs what the *line* sees where such a node stands, so I can write your answers into DESIGN and resume it. Each point stands alone.

**"does this actually work?"** Yes. The `return` node keeps what it returns in its field, and its run leaves the function with that value; the function's `-> T` checks it. The node stays in the graph, so nothing is lost.

## 1. My reading of your answers, to be written into DESIGN as you rule them

1. A node with no `output_type` gives its line nothing. So `y := (x := i32 5)` is the error of binding nothing, as `a = b = c` already is. `y := while false (1)` is an error, and `(return x) + 1.0` is refused where it is written.
2. `return` is its own node type with one field, `dyad ?`, holding what it returns: `5.5`, `f64 43`, a type or a `fn`.
3. An `if` node holds its `else if` branches and its optional `else` in its own fields, as a list behind a pointer, since it grows while it is parsed (›Operands sit inline after the type word; a growing list stays behind a pointer‹). `else` and `else if` are no nodes of their own. An `if` without `else` is the same type `if`. `while` and `for` are their own types too, each with its own fields.
4. An `if` gives a value only when every path gives one and every arm gives the same type. So `y := if false (5)` is an error, binding nothing, and `if c (i32 1) else (f64 2.5)` is refused where it is written. A plain number is converted by the other arm's type, as in `a + 1`: `if c (1) else (f64 2.5)` is an `f64`.
5. Arms whose type is known only when the program runs give a `dyad`, and each later use is parsed when the run gets there, as ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹ already does for a held type known only at run:
   ```logos
   mut d := dyad ?, mut e := dyad ?, d = i32 1, e = f64 2.5,
   x := if c (d) else (e),
   print «{x + 1}»          # parsed when the run gets here, with x's type then
   ```

**Answer 1:** 
I have to improve the rule on all branches inside if needing to return the same type. that is actually wrong. they all need to be convertable to the same type which is required outside the if section. 

For the code example that is a good example and actually as much as possible should be parsed but only lines that call x cannot be completely parsed but they can be parsed all the way until the constructor of x is trying to be called because the constructor could be anything. but it should only affect things on that line so all later lines that doesnt call x can also be parsed beforehand. 

## 2. An arm that leaves the function

```logos
f := fn (c := i32 ?) -> f64 (
  x := if c == 1 (return f64 0.5) else (f64 2.5),
  x + 1.0
),
f(2)     # today 5e-324, should be 3.5
```

The `return` arm gives nothing (1.1), and the `else` arm gives an `f64`.

- (a) **Only the arms that reach the `if`'s end must agree.** An arm whose last line is a `return`, `error.X «…»` or `abort «…»` leaves the function and never reaches it, so `x` is an `f64`.
- (b) **Refused:** a `return` arm gives nothing, so the arms differ. The program writes `if c == 1 (return f64 0.5), x := f64 2.5`.

Recommended: (a). It is the common shape (an early exit in one arm), and it needs no new type word.

**Answer 2:** 
ok i see now. return should have a run body that runs whatever its dyad field runs but it runs it so its output is assinmed to outside the function where the function is called. but this means that other types with output_type needs to somehow explain that its writing its output to what ever consumes it. is this alreayd solved though? so return should basically also have a run body but it always writes the output to outside the function. 

## 3. `a := return x`

You wrote that it "would work but a would become the return … a and return x are synonymous".

- (a) **Refused where it is written**, as an `=` in a value position is (›`=` sits beside `:=`, and returns nothing‹). A line's node can already be read without running it, through its scope's `dyads` (›Reading a path runs nothing‹).
- (b) **`a` names the node**, so writing `a` later runs `return x`. Every `x := …` whose right side gives nothing then means "name this node" rather than the error, `y := (x := i32 5)` included.

Recommended: (a). With (b) a typo (a statement where a value was meant) would quietly make an alias instead of an error.

**Answer 3:** 
based on by previous answer i would go for a here, but just so you know it i want to make all LG assignable in the future because it allows for more meta programming. but for now we can make := = return etc not possible to assign to some variable

## 4. Minting, as one rule

You said `+ - * / ^` and `type` mint, and `if`, `return` and `while` do not, without knowing yet when to mint. One rule gives exactly your list. It is the rule you ruled for data types on 30 September, without its "without a `run`":

> A type whose field is `?` mints one type per set of field types, with or without a `run`.

- **`+` and `^`** declare `lhs := ?, rhs := ?` (identities/power.logos), so they mint: `+` over `i32` is one type, `+` over `f64` another.
- **`if`, `while` and `for`** have fields of fixed types, and **`return`** has a `dyad` field (1.2), so they do not mint.
- **`:=` and `=`** mint if their `rhs` is declared `?`. Each mint's `run` then says how its type is written: your "per type what := and = is actually writing".
- **`type`:** a type whose field is `?` already mints (›A type without a `run` whose field is `?` mints one type per set of field types, as `array` mints one per element type‹). That rule's Open line asks exactly this question for a type with a `run`.

What it changes:
- `output_type` becomes `share output_type`, written once when the mint is made, not into every node.
- A `+` node is `[+ over i32, LHS, RHS]`: its type word is the mint, chosen by `+`'s parse, with no operation word and no output word.
- The lowering rule stays and maps the mint `+ over i32` to `add_i32` per backend.

Three rules say otherwise today:
- ›A node's output type is per node, and its parse writes it‹: "The author declares `output_type := type ?`; the parse writes `tape[0].output_type = tape[-1].type`". Its Rejected line, "one result type fixed on the type (not dynamic enough)", still holds, since `^` over `i32` and over `f64` are two mints with two output types.
- ›Operands sit inline after the type word; a growing list stays behind a pointer‹: "`[+, LHS, RHS, type]` … a lowering rule fills it, never the operator's constructor at parse".
- ›Which machine operation an operator is (`+` over `i32` → `add_i32`) is a lowering rule‹: "the node's type is `+` either way, which reflection and rewrite patterns read".

One consequence: a rewrite rule written `a + 0` must match `+` over every type. On 18 September you declined `a.type is array` for arrays ("isn't it enough for always?"), because generic code writes `array T` with a hole. The same holds here: a pattern written over holes matches every mint of `+`.

- (a) **The one rule above**, with the three rules changed to it and `^` in power.logos declaring `share output_type`. The #82 slice still decides a node's type once at parse and reads it from one place, and that place is the mint.
- (b) **Not yet.** The rules stay as they are, with an `output_type` word in every node (Q-59.5 (a)), and minting is asked again when you know the rule.

Recommended: (a). It is one rule you already have, it answers its Open line, and no list of which types mint has to be kept.

**Answer 4:** 
For the case fo rhs := ?, lhs := ? their types must always be the same since the instructions needs them both to be the same, right? im quite sure the inputs for instructions needs to have the same type. since the lhs and rhs types are the same the output type is also the same therefore the mints should be per such type and lhs and rhs should be declared with that type which is the same as output type. since for the run body to be able to parse the types of all fields must be kown. and i know this is maybe constrdicting the ruling but im quite sure that all fields that is used inside run should have be declared with a type. that is sort of the reason why we are doing minting. its so that the run body can parse at first. btw when lhs and rhs are different types the parse which runs before the mint is created must decide which type to chose of lhs.type and rhs.type and when minting one of lhs or rhs has to have a conversion step so for "a + 0" 0 which is the type rational_number must most likely get a conversion step and that conversion step becomes the rhs of that mint of +. the conversion must in this specifi case convert to a.type, if a is a type other than rationl_number. this means the parse function for + needs to be somewhat large most likely. but that is true for all other types which does minting as well. 

## Metadata
- **Status:** answered, relayed 2026-10-02 15:29
- **Priority:** #82 is the root every other fix waits for, and its worker is resumed only with these answers; 1 is one "ok", 2 to 4 one letter each
- **Asked:** 2026-10-02 14:17
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#82](https://github.com/ThobiasKnudsen/LogosLang/issues/82), first slice: every node carries the type it gives back
- **Branch and worktree:** `issue-82-output-type`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-82)
- **Asked by:** Orchestrator (2), from your Q-59 answers (Q/A on #82, comment 5952123021)
- **Waiting:** #82's worker, whose job ended with the restart; it is resumed with Q-59's and these answers once they stand in DESIGN
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [05_Q-59_issue-82.md](../history/Q-59_issue-82.md): your answers this round builds on
  - [DESIGN.md](file:///home/o/Personal/Code/LogosLang/DESIGN.md): the rules quoted above, and ›A scope's value is what it evaluates to, and `return` is an optional early exit from the enclosing function‹
  - [power.logos](file:///home/o/Personal/Code/LogosLang/identities/power.logos): `^` declares `lhs := ?, rhs := ?, output_type := type ?`
