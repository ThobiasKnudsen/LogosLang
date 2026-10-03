# Q-14: seven points your transparent-`dyad ?` ruling leaves open

## Metadata
- **Status:** answered, relayed 2026-09-29 22:15
- **Priority:** 2 (why: the #199 worker is idle until you answer; each point has a recommendation, so "all recommended" answers it, but it is the longest read in the folder)
- **Asked:** 2026-09-29 21:22
- **Project:** LogosLang, [repo](file:///home/o/Personal/Code/LogosLang)
- **Issue:** [#199](https://github.com/ThobiasKnudsen/LogosLang/issues/199), the same text is its comment 5896965799; compile question [#201](https://github.com/ThobiasKnudsen/LogosLang/issues/201)
- **Branch and worktree:** `issue-199-bare-param-dyad`, [worktree](file:///home/o/Personal/Code/LogosLang/.claude/worktrees/issue-199)
- **Asked by:** logoslang-issue-199-worker (session 6dd47466), Opus 5.5
- **Waiting:** the agent is idle until this is answered
- **Orchestrator:** Orchestrator (2)
- **Related:**
  - [your answer](../history/Q-11_issue-199.md): "define dyad so that its transparent and a placeholder for a new node with any type"
  - [the other open question](../history/Q-13_issue-171.md): which root next; answer that one first

<!-- Write your answer on the **Answer** line under each question, or anywhere else in the file, and save. "all recommended" also works. -->

Your ruling is recorded (commit e2415a8): a `dyad ?` place is a transparent placeholder for a node of any type, so `f := fn (a) -> i64 ( a ), f(i64 42)` gives `42`.

## 1. Does the transparent `id` node you rejected on 28 September stay rejected?
On 28 September you rejected "a transparent `id` node" for names whose type is unknown while parsing, because a body is built per argument set anyway and an extra level "would also make things less intuitive when inspecting the LG". Today's rule is close to it.
- (A) **Stays rejected.** The transparent dyad appears only where someone writes `dyad ?` or a bare parameter, for types known only at run.
- (B) **Superseded:** any name of unknown type may be a transparent placeholder.

Recommended: (A); `?` is typed per build, `dyad ?` at run, so they do not overlap.

**Answer 1:** 
well i didnt consider this case where the type could be anything. but the name id should stay rejected but the concept that dyad can be transparent should not be rejected. in the case where the type is given when the function is called does not make it unintuitive that the type is dyad actually. btw these things you tell be about that the type is given at parse anyways tells me that in some cases the argument section in the definition of a function is lexed first then parsed per call site. is that correct? 

## 2. What does `a:type` give?
```logos
f := fn (a) -> bool ( a:type == i64 ), f(i64 42)
```
- (A) **`true`:** the type of the node `a` holds; the dyad is transparent for `:type` too.
- (B) **`false`:** `a:type` is `dyad`, and the held node's type is read some other way.

Recommended: (A).

**Answer 2:** 
A

## 3. When is `-> i64 ( a )` checked?
```logos
f := fn (a) -> i64 ( a ), f(i64 42), f(f64 2.5)
```
- (A) **When the body runs:** `f(f64 2.5)` is a run-time error. `fn (a := ?)` stays the form checked while parsing.
- (B) **At the call, while parsing,** where the call knows the type; at run only where it does not.

Recommended: (A), a clean split the author picks by what they write.

**Answer 3:** 
in the case of A it should actually show error when prased since that is when its surfaced but in some cases the type is known at runtime but that runtime has to parse first so it should error at parse anyways. so looks like B is correct 

## 4. Does the argument run before it goes into `a`?
```logos
x := i64 5, f := fn (a) -> i64 ( a ), f(x + 1)
```
- (A) **It runs first:** `a` holds `i64 6`, and `f(x + 1)` is `6`. A field that must keep its operand unrun then names the node kind it takes (as `elements := square_brackets ?` does); `identities/proof.logos`'s `pattern := dyad ?` and `replacement := dyad ?` need such a type, which you name.
- (B) **It is kept unrun:** `a` holds the `+` node, and `a:type` is `+`.

Recommended: (A).

**Answer 4:** 
it should run the exact same way as a := x + 1. those are synonymous. its just different syntax. its also synonymous to (a, b, c) := (x, y, z). that is how function arguments should work. it should therefor be an LG derived from x + 1 so its not immediate. if you want it immediate you would have to state so explisitly. so option B is correct. 

## 5. A second write into a dyad that already holds something
```logos
mut d := dyad ?, d = i64 5, d = f64 2.5
```
- (A) **The first write sets the type;** the second writes into the `i64` node, and `f64 2.5` is refused.
- (B) **The write replaces:** a new `f64` node, the old one freed first.

Recommended: (A); with (B) a name's type can change halfway through a body.

**Answer 5:** 
B based on earlier reasoning

## 6. Which type does a bare literal get in an empty dyad?
```logos
f := fn (a) -> i64 ( a ), f(42)
```
- (A) **It stays a literal until the tail commits it:** `f(42)` is `42`, `f(1/2)` is refused.
- (B) **Refused:** write `f(i64 42)`.

Recommended: (A); DESIGN commits a literal "when it lands in a typed slot", and the tail is where it lands.

**Answer 6:** 
literal should also have a type so e.g. «example» is a string while a plain number is type rational_number. there should be specific operations you can do on rational_number type.

## 7. What does `compile f` do with such a function until #201 is settled?
- (A) **Refuses it with a clear error;** it runs interpreted until #201 decides.
- (B) **Compiles it with a type check at every read:** slow but working.

Recommended: (A).

**Answer 7:** 
i dont see what context this question is asked. 
