# Q-22: three points your `.type` ruling leaves open

## Metadata
- **Status:** answered, relayed 2026-09-29 23:49
- **Priority:** 2 (why: point 2 decides what the `.type` rename reads, which blocks filing that change as ready; nobody sits idle, the review of the recorded answers runs meanwhile)
- **Asked:** 2026-09-29 23:18
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), a bare parameter `fn (a)` is a `dyad ?`, which holds a node of any type; your answers there also moved a value's type from `a:type` to `a.type`
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-worker (session 6dd47466, ended), Opus 5.5
- **Waiting:** nobody is idle; the `.type` rename is drafted and waits for point 2
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your four answers](../history/Q-18_issue-199.md): what these follow up
  - [the #197 points](../history/Q-19_issue-197.md) and [the `i32 m` points](../history/Q-20_issue-212.md): unrelated

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. "all recommended" also works. -->

For your information, no answer needed: your `.type` answer reverses an August rejection of "the universal `.type` metaproperty on every value" (›The dyad's read surface‹), whose reason was "a value's type is never one of its own fields". Your reason, that the type now lives in the same payload as the value, answers it, so the worker recorded the August rule as superseded. Say if not.

## 1. Where does the reflection gate stand, now that a type is read with `.type`?

DESIGN ›Reflection (`:type`) is fail-closed and gated twice‹ says reflection is the `:type` read: "a section that cannot spell `:` cannot reflect at all", and an imported file "stops at a private name's `:type`". Now the type is an ordinary `.` field, so a file without `:` can still write `.type`:

```logos
p.type     # in a file that holds no `:`: was refused as `p:type`, now an ordinary field read
p:start    # still refused there: the graph where p was made
```

- (a) **A value's type is not reflection: it travels with the value.** The gates stay on the `:` reads that remain, `a:start` (the way into the graph) and `a:dyad`.
- (b) **`.type` stays gated:** `type`'s body declares it with the same two gates.

Recommended: (a), because what visibility hides is how a value was made, and that is reached by `:start`.

**Answer 1:** 
there can still be a gate that says you cannot access the type field though, but that gate doesnt exist now as im aware. the same way you can gate other . fileds. 

## 2. What does `.type` of an expression nobody named give?

Rewritten at 23:45 by #199's reviewer: your `.type` answer settled the spelling, not this. An expression can meet `.type` in two ways, written in code or reached as an operand inside a type's `parse` body (`tape[-1]` is the operand to the left):

```logos
(x + i32 1).type                          # written in code
tape[0].output_type = tape[-1].type,      # in ^'s parse for (x + i32 1) ^ 2: tape[-1] is the + node
x := i32 3, (x + x).operands[0]           # 3 today: `.` on a written expression reads the node
```

- (a) **What it yields: `i32`.** ›A node's output type is per node, and its parse writes it‹ needs this (`tape[0].output_type = tape[-1].type`), and ›A value of a type built by a Logos `parse` travels as a pointer‹ says "a line's `:type` is the type of what it yields". #209 (a type's `run` body picks its build by a list that knows only number types) and #82 (the one reading rule) need the same read. Then `(x + x).operands[0]` needs another spelling.
- (b) **The node's own type word: `+`.** ›`:` reads a name's binding…‹ says "`b:start.rhs.type` reads the `+` node's type", and ›The dyad's read surface‹ reads `.` on a written expression as the node: "`(x + x).operands[0]` reads the first operand". What an expression yields is then `.output_type`, which a name or a number does not have, so `^`'s parse needs two reads.

Recommended by the worker and the reviewer: (a), so the named and unnamed forms of one value give one answer.

**Answer 2:** 
btw (x+x).operands should not exist. it should be lhs and rhs for all the other operators as well. but this question is kind of hard though because in some cases you want to get '+' type and in other cases you want to get the 'i32' type. but im leaning towards a here where .type reads the fields of what is evaluated, not the LG. to rather get '+' type instead you would need to do something else which im not sure of how right now. byt .type is consistent with the fact that you reaches the value field which is 3 here the same way by just writing (2 + i32 1). if f returns a type with the fields x and y then f().x would access that value the same way, not reflecting on the LG. 

## 3. Which type keeps an operand as graph, now that `dyad ?` holds the value?

DESIGN still says "A node-typed field keeps the operand as graph (how control-flow words are written with `run`)", and that type was `dyad ?`. `identities/proof.logos` declares a rewrite rule's two sides that way, and they must stay expressions, not values:

```logos
proof := type ( pattern := dyad ?, replacement := dyad ?, … )
```

- (a) **`@dyad ?`:** a pointer to the operand's node keeps it as graph, and reading through it is explicit. `language_sketch.logos` already writes `@dyad ?` for "a node of the list".
- (b) **A type per kind of operand:** `scope ?` for a bracketed one, `square_brackets ?` for a list (as array.logos's `elements` is), and something still to name for any other expression.

Recommended: (a), because it covers every operand, a proof's pattern included, with a word that already exists.

**Answer 3:** 
a