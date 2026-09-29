# DESIGN_HISTORY.md

**What this is.** The archive of DESIGN.md: the History lines, the superseded-record rules, the seed detail and the extra quotes that left DESIGN.md on 27 September 2026, keyed by the same `##` sections and `###` rule headings, so a rule's past is found under the heading DESIGN.md gives it. Under a heading, **History** holds the moved History bullets verbatim, **Superseded record** a rule deleted whole, **Seed detail (27 September 2026)** the seed status as it stood before DESIGN.md's Seed line was shortened, and **Quotes** the passages of Thobias's speech that left the rule, each marked with the line it came from. `DESIGN.md l.N` in it means line N of the paragraph form at git commit e75bcdc. Nothing here rules; DESIGN.md does.

### Versions are named vX.Y.Z
- **History:** *v1* and *Core* respelled by meaning, 5 September 2026. (l.89 still says "Core".)

## The core vision

### Interpreted by default; any function can be JIT compiled, mutable code included
- **History:** until 27 September 2026: "an immutable (frozen) region can be explicitly JIT compiled … whereas mutable code stays interpreted". Dropped because it disagreed with ›Mutable code is compilable‹ and with the 7 September 2026 ruling that there is no frozen state.

## Why now: a substrate for machine-written code

Nothing moved from this section.

## The architecture

### A dependency cycle among core identities is broken with native versions first
- **Seed detail (27 September 2026):** the 23 Sept text calls `hashmap` "a native the seed owes, pending"; the 25 Sept seed note (#137, below) describes it as present.

### A declared hashmap is the empty map: `m := hashmap K -> V`
- **History:** first a top-level `array_mints := …`; inside `get_mint` with the mark since 25 September 2026. The mark `shared` was renamed `share` on 26 September 2026 (l.207).
- **Seed detail (27 September 2026):** (#137) `hashmap K -> V` places a fresh empty map, and the interned type only where `?` follows, so `(w := hashmap K -> V ?)` stays a parameter. The type is read off a map: `m:type` or `(hashmap K -> V):type`.

## The rewriting engine

Nothing moved from this section.

## The type strata and verification

### Top tier: any mathematical statement about a program is statable and provable
- **History:** was "full dependent types and proof terms: types depending on values, propositions as types, proofs as programs, checked by a small trusted kernel". Superseded in discussion, August 2026: statable-and-provable stands, the propositions-as-types shape does not.

### A rewrite rule is its own proof, so verification composes with rewriting
- **History:** first "each rewrite rule can carry a proof"; August 2026: the rule does not carry a proof, it is one.

## The proof layer

### A conjecture states a boolean, and calling it yields the other side of a fact
- **History:** supersedes the `->` mapper (the statement as `pattern -> replacement`, "the left side is what is recognized", "two recognizers, two conjectures"), "a chain of `->` steps", "the relation a mapping preserves is fixed by what it maps" with the `S -> true` truth-rule form and goal reduction, the rejection of a relation symbol between derivation lines, and "the truth rule is the primitive". Those rules stand below as the superseded record. `->` stays where it is a type arrow, `fn (…) -> T` and `hashmap K -> V`.
- **Seed detail (27 September 2026):** nothing; the proof layer is v1.0.0 standard-library work.

### A conjecture is statement, context, evidence (superseded record, 27 September 2026)
- **Superseded record:**
  `half := conjecture ( (a + a) / a -> 2 ) where ( a:type is number and a != 0 ) proof ( … )`
  - The statement holds `pattern` and `replacement` as one `->`. **Left side: what is recognized. Right side: what it becomes.** `a + a -> 2 * a` recognizes the sum and yields the product; `a + a == 2 * a -> true` recognizes the equation and yields `true`. Two recognizers, two conjectures, never one object read two ways.
  - `where` holds **one boolean**, the `premises`. A hole's type is a premise, not a declaration: `a:type is number`, `is` being membership in the collection of numeric types (›Element access is `[…]`‹).
  - `proof` holds the `derivation`. The `world` is computed.
  - **Ruled:** in discussion, 6/7 September 2026; left/right precision and `is` 8 September 2026.
  - **Rejected:** `== generic_number` (`==` on types is identity, and inheritance is rejected); `where (a := generic_number ?, …)` (a hole's type is a premise).
  - **History:** `a:dyad.type` respelled `a:type` 23 September 2026. August spellings superseded by this form (see ›A proof is its own type‹).
  - **Source:** DESIGN.md l.69

### A derivation is a chain of `->` steps (superseded record, 27 September 2026: a chain of relations, see the first rule of this section)
- **Superseded record:**
  `->` is the *mapper*: directed, "convert this form into that", target named. A boolean operator is a *relation* and names no target, so only `->` stands between lines. Each right side holds one call of the cited conjecture at the position it applies, its argument the pattern instance, checked against the previous line's reduced form. A line with no call is an identity step showing the reduced form. The trailing form is the replacement:
  `proof ( (a + a) / a -> double(a + a) / a -> mul_div_assoc((2 * a) / a) -> 2 * div_self(a / a) -> mul_one(2 * 1) -> 2 )` (`div_self`'s premise `a != 0` is `half`'s own).
  - **Rejected, to stay rejected:** a relation symbol or implication symbol between lines. The mapper is what a step is; the relation it preserves is the cited rule's.
  - **Ruled:** in discussion, 6/7 September 2026.
  - **Source:** DESIGN.md l.69

### The relation a mapping preserves is fixed by what it maps (superseded record, 27 September 2026: steps carry their relation and compose by the table, see the first rule of this section)
- **Superseded record:**
  - Over **values**: equality. A step rewrites a subterm anywhere; accepting the conjecture merges its two sides into one equality class (the engine's *known equal*).
  - Over **booleans**: goal reduction, the left implied by the right. `a <= c -> (a <= b and b <= c)` reads "a <= c reduces to those two". A truth rule `S -> true` is a chain from S down to `true`: `x <= x^2 + 1 -> 0 <= x^2 - x + 1 -> 0 <= (x - 1/2)^2 + 3/4 -> true`, each step citing an iff rule, a value rewrite inside the line, or a rule whose right side is `true`, as written by hand.
  - No relation slot. "A implies B" is the reduction `B -> A`, or the truth rule `(A => B) -> true` cited through modus ponens.
  - **Why:** it follows from the recognizer reading with nothing added (for a value the sides are equal; for a boolean the right side is what is left to show). Goal reduction is the direction the coherentist backward search already walks, so written and searched proofs read alike. A relation slot would add a spelling and a composition table for a case the boolean chain covers.
  - **Ruled:** 8 September 2026.
  - **History:** superseded 8 September 2026: the composed relation ("`==` with `<=` is `<=`", `x <= x^2 + 1` proved by mapping `x` to `x^2 + 1`) and "steps cite truth rules only: a value rule is the reading of an accepted equation, never a second object".
  - **Source:** DESIGN.md l.69

### `where` is checked where it can be
- **History:** "fn a type, proof a shape" superseded: both arrows are followed by a shape, a bare type being the shape "a value of that type".

### A proof is its own type, merged with `fn` in behaviour, never in structure
- **History:** superseded propositions-as-types (August 2026). Superseded August spellings: `p1 := proof (a := generic_number ?, a + a) -> (2 * a) ( … )`, `proof (a := generic_number ?, a + a == 2 * a) -> true ( … )`, `proof (a := generic_number ?, a != 0, a / a) -> 1`.

### Truth rules and value rules (superseded record)
- **Superseded record:**
  A **truth rule** states any proposition whatever.
  - **Ruled:** 27 September 2026, Thobias: dissolved. A conjecture is one boolean statement, and calling it yields the other side of a fact (›A conjecture states a boolean…‹); there is no value rule beside a truth twin.
  - **History:** l.71 (August 2026): "The truth rule is the primitive: a value rule is accepted exactly when its truth counterpart holds." l.69 (6 to 8 September 2026): "two recognizers, two conjectures, never one object read two ways". Both superseded.
  - **Source:** DESIGN.md l.69, l.71

### Application is the rewriting engine unchanged; matching is fail-closed
- **History:** wording amended in discussion 30 August 2026: "by address" is the case with nothing yet proven.

### Proofs are a layer over the Logic Graph, not a property of the substrate
- **History:** superseded 30 August 2026, because proofs as rewrite rules involve no type of types: the `universe_of` classification stored per node beside `type_of` (the Lean/Coq universe treatment); the predicative tower `Universe 0 : Universe 1 : …`; `Prop` as impredicative, proof-irrelevant, erased before codegen; connectives (`False`, `And`, `Or`, implication, `Forall`, `Exists`, equality) as inductive and function types; Curry-Howard at the representation level; kernel obligations (checking stored universes, strict positivity, definitional equality only on the terminating tagged fragment, no elimination from `type : type` into `Prop`). The June 2026 open question (proof-irrelevant `Prop` needing the large-elimination restriction, or proof-relevance) is closed by the proof-relevant reading above.

### The sound layer can reason about any node, but not its own consistency
- **History:** the same paragraph defined a theorem as "a definition whose type is a proposition and whose value is a kernel-checked term" and an axiom as "a proposition declared with an empty value slot": propositions-as-types, superseded August 2026 (l.71). Now theorem/axiom/open are states (›No proof means derivation `?`‹).

### The proof layer ships in v1.0.0's standard library, not in the seed
- **History:** ›Feasibility‹'s "not needed for an initial release", superseded 5 September 2026.

### The signature is a pattern: an unknown spelling in it is a hole
- **History:** until 29 September 2026: "the spelling enters the trie pointing at a fresh null-slotted dyad". The fresh dyad went with the one-word node (#166, ›An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value‹): the spelling enters as a binding with no node yet; the sentence followed on 29 September 2026 (review round 2 of #166).

## The ecumenical proof system

### Branches and universes (superseded record)
- **Superseded record:**
  - **Ruled:** 27 September 2026, Thobias: l.99 superseded. Branches stand alone; the universe remark returns only if the paradox case of ›Proofs are a layer over the Logic Graph‹ ever appears. Reason: the 30 August ruling already says when stratification comes back.
  - **History:** l.99 (June 2026 direction): "It composes with the universe kernel rather than replacing it. The two are orthogonal layers over the one graph: *universes* stop self-referential paradox **within** a theory (the stratified `universe_of` tower above); *branches* manage incompatible assumptions **across** theories. They meet at one productive point: large-cardinal axioms, which assert that taller universes exist, so there 'which branch am I in?' and 'how tall is the universe tower?' are the same choice, settled (as in real mathematics) by fruitfulness, i.e. by the coherentist layer." l.75 (ruled 30 August 2026): "the `universe_of` tower, `Prop` as an erased proof-irrelevant universe, and the June proof-relevance question below are no longer live spec … Stratification re-enters only if rules quantifying over rules … is shown to admit a paradox, not before."
  - **Source:** DESIGN.md l.99, l.75

## Memory and concurrency

### Explicit heap: `alloc n` and `alloc n of T v`
- **History:** heading ›Explicit heap, and no implicit destruction: `alloc n` and `alloc n of T v`‹ until 28 September 2026, when ›A value's teardown runs where its life ends; the ending identity reads the type's `free` slot‹ made the type's `free` run by the identity that ends a value's life.
- **History:** first spelled `alloc T v`, returning an owning `@T`.
- **Seed detail (27 September 2026):** since 23 September 2026 the span's byte count sits in a header before the first cell, and `free` reads it back.

### A value's teardown runs where its life ends; the ending identity reads the type's `free` slot
- **History:** supersedes ›Nothing is destroyed implicitly: the constructor writes the teardown as `defer`‹ (settled July 2026): the constructor wrote the teardown and inserted it as `defer` structure at the binding site, `alloc` inserting `defer free a` naming the specific allocator, any constructor likewise (`defer close`), last-in-first-out at scope exit. Its Why: only the construction site knows how a value was built, which allocator, whether ownership was taken; an `@T`'s type could not carry that. Its "Rejected, to stay rejected: implicit scope-end destruction in any form; `:=` inserting `defer drop` on every declaration". Overturned 28 September 2026 by Thobias: implicit as `run` is implicit, the ending identity reads the slot; the owning `@T` is its own type since 25 September 2026 and the allocator lives on the value, and every end away from scope exit had become a patch on the inserted node (#170 the patch never written).

### `move` is the act, `own` the gate word, `free` the end
- **History:** the act was `take` until 30 August 2026, then `own` for act and gate alike until 28 September 2026; the end was `drop` from July 2026 until 28 September 2026. Each rule whose heading carried the old words keeps the old heading in a History line here.

### A type whose fields carry teardowns must write its own destructor
- **History:** at first a record type's auto-derived constructor composed its fields' teardowns; per-type order 30 August 2026.
- **Seed detail (27 September 2026):** composing field teardowns waits on the destructor-authoring check.

### `free` and `move` end a name; no drop flag
- **History:** heading ›`drop` and `own` end a name; the teardown is removed or moved at parse; no drop flag‹ until 28 September 2026; the moving and removing of an inserted teardown node at parse retired with the insertion itself (›A value's teardown runs where its life ends; the ending identity reads the type's `free` slot‹), nothing standing at the declaration to move.
- **History:** first a sanctioned no-op over a null drop flag, "all with no graph edit"; replaced 7 September 2026. `own` as an access kind: gone 7 September 2026. The null stand-in, binding-site attachment and the `share` direction: settled in discussion, July 2026.
- **Seed detail (27 September 2026):** stand-in: a place is emptied by writing a **null pointer**, and the scope-exit teardown does nothing on null, a run-time check the ruled model lacks; removing it is pending. `alloc`/`own`/`drop`/`free`/`defer` exist; the teardown runs last-in-first-out at scope exit as body structure. The owning heap pointer is the first identity with a non-null `drop` slot; a `&x` borrow mints the same `@T` with a null destructor, so owning-ness rides on the node `alloc` built, not on `@T`.

### Holding is decided at the binding site, parameters included
- **History:** heading ›Teardown attaches at the binding site, parameters included‹ until 28 September 2026, when `defer free <place>` stopped being inserted; the binding site still decides who holds.
- **Seed detail (27 September 2026):** covers only *named* owning bindings. A bare owning temporary passed as an argument is rejected: a bug against the ruling.

### Three fail-closed ownership rules, and `-> own @T`
- **History:** "ownership may not cross a function return at all" and "a bare `-> @T` returning an owned place stays the checked error": superseded for a last value 25 September 2026.

### A pointer steps by whole cells
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): pointer on the left, integer on the right, k scaled to bytes as an `i64` product in the graph, both tiers. `k + p`, `p - q` and every other pointer operator stay refused.

### A filled `share free` is the value's teardown; one name owns each value
- **History:** heading ›A filled `share drop` is the constructor's teardown; one name owns each value‹ until 28 September 2026; `defer drop a` was inserted where ownership landed, now the holder's scope runs the value's `free`.
- **History:** text spells the mark `shared drop`: renamed `share` 26 September 2026 (l.207). Text says "fields block fills": the fields block was removed 25 September 2026 (l.203); members stand in the type body. Text uses `this.ptr` and "`this` bound to the instance": `this` removed 26 September 2026 (l.207).
- **Seed detail (27 September 2026):** since 25 September 2026. The owner's binding carries `own` in its gate set, read by `a:gate` and consulted by `own a` and `drop a` (Claude's choice, open to Thobias: the gate set is where a name's other facts live, and it outlasts one REPL line). A move writes the null stand-in, and `free ptr` in the drop empties the field; so a borrow's `b[0]` after the owner's drop is the checked null-pointer error, while `b[1]` steps a cell past the null and faults (the hole in full). Not freed yet: a value that moves out and reaches no name (`mk()` as a statement); a value just made passed straight to a call (`f(array i32 (1, 2))`). A value a type's own `parse` places as the node itself, not a call on it, gets no owner (text: "places as `this` itself"). `own` on a parameter, which would consume the argument, is not built: checked error meanwhile.

### A field may be `own @T ?`
- **Seed detail (27 September 2026):** `own` in a type takes a pointer hole on a field or a name: `mut a := own @i32 ?` frees what is written into it.

### A field may be `own t ?`, `t` a type whose body fills `share free`
- **History:** heading spelled `share drop` until 28 September 2026. Open until then: `=` into an owning field leaked the displaced node (the seed still does, #170); ruled 28 September 2026 that the displaced node is freed before the write.
- **History:** text writes `this.items` and "fields block"; see renames above.
- **Seed detail (27 September 2026):** since 25 September 2026, no array-specific code; `drop items` and `own b.items` run interpreted like every field read. Closed the note "`own array i32 ?` is not in the seed yet".

### `move` and `free` are static: the parse marks the name dead
- **History:** heading ›`own` and `drop` are static: the parse marks the name dead‹ until 28 September 2026 (›`move` is the act, `own` the gate word, `free` the end‹); rule 2 relocated an inserted defer node into the non-moving arms, now the `if` runs the `free` there.
- **History:** maybe-moved reading ("the phase bit still decides at run time whether the teardown fires"): superseded 7 September 2026, since every legal `own` ends a lifetime at a point the parse can name.

### A call is a use of every outer name the callee's body reads
- **Seed detail (27 September 2026):** since 15 September 2026 (#125): the list is the fn value's trailing `outer` slot, filled by the body's parse (identities it dispatches, operands it takes, once each), read wherever a node that runs a body comes to exist: a call, a node of a `run`-carrying type (applied or built by its constructor), a Logos-written constructor's run. A name from a section on no caller's stack counts as live: an imported `pub` function reads its private siblings after the section's parse ended, and importers share the one loaded scope (›Importing is dropping the text there‹). A scope that the code asking for a body stands in (a held `type (…)` built when its function runs, a run body built for a field-type set) counts as open while that body is built, since that code runs inside it: Claude's fix 25 September 2026, open to Thobias (`array bag` built the mint's held body in array.logos's scopes alone, so `bag`, whose `parse` calls a `fill` reading the command line's `array`, was refused "`array` is not in scope here"). Callee outer names join the caller's list.

### `free x` works on any identity
- **History:** ›`drop x` works on any identity‹ until 28 September 2026.
- **Seed detail (27 September 2026):** realized the same day.

### A `move` argument is consumed at the call; a callee that does not take it hands it back in its error value
- **History:** ›An `own` argument is consumed at the call; a callee that does not take it hands it back in its error value‹ until 28 September 2026.
- **History:** "on failure ownership stays with the caller": superseded 3 September 2026, since `own` is static and the name is dead from the call. In v0.1.0 a failed call is a fault, so the case does not arise.

### `move` and `free` take a field path too
- **History:** ›`own` and `drop` take a field path too‹ until 28 September 2026.
- **History:** 3 September 2026: bare names only (a value owned whole or not at all; `own p.x` a parse error), because a field is bytes at an offset, not a dyad, with no phase bits for a moved flag. Superseded 5 September 2026, and the swap door with it. "Guarded by the field's phase bit where it does not": superseded 7 September 2026. Closed 7 September 2026: how a non-pointer field carries a phase bit (none does; the teardown moves instead).
- **Seed detail (27 September 2026):** accepts `own p.f` and writes the null; lacks the sub-range, the dead-as-whole mark and the relocation: pending work, no longer a bug.

### A name's gates are one ordered set on its binding, combined as grants and vetoes
- **History:** 7 September 2026 morning: "all of which must pass"; grants and vetoes the same day. Two default entries at first, write "while the slot is null" for the own scope and below; default write removed 20 September 2026. `mut` first allowed writes from the own scope only ("a `pub` name stays writable only where it was declared"); unscoped 20 September 2026.
- **Seed detail (27 September 2026):** `mut` since 20 September 2026; the gate set is an array on the binding; the declare node's gate slot is gone. Writes through a field path or a dereference are not gated yet.
- **Quotes:** Why: "mut just says its mutable, not where its mutable, while pub makes it public".

### A gate is a node of the body; the binding's set is a cache folded from those nodes
- **History:** "one ordered set on its record" (7 September 2026) stands as what the binding holds, no longer as where the truth is.

### The check is lexical and static: no ambient authority
- **History:** "erased before codegen" amended 5 September 2026 to "resident, never resolved by running code".

### Two access kinds: read and write, on a path
- **History:** first an open set: `call`, `own`, and "more (borrow, alias, drop, observe-for-proof) admitted the same way"; superseded 7 September 2026. Text writes `f.code` (slot renamed `run` 17 September 2026, l.189) and `x:dyad` (`:type` since 23 September 2026, l.108).

### Effect signatures keep the classification honest; gates run on the stack they analyze
- **Seed detail (27 September 2026):** the chokepoint, the lexical tier and the classified `?` seam only; **no prover**. Borrow checker and proof layer bolt onto `?` later.

### Gate spelling: words left of `:=`; ownership in the reference type
- **History:** moved right, `x := pub mut 5`, 5 September 2026 (the left of `:=` was a name alone); back left 15 September 2026, since a gate is an entry on the name's binding and the left of `:=` is the name with its gate words (›Declarations are immutable by default‹). Ownership words respelled with the one declaration operator 2 September 2026. "The reference's gate grants `own`" respelled 7 September 2026.

### Reflection (`:type`) is fail-closed and gated twice
- **History:** `:dyad`, retired for `:type` 23 September 2026.
- **History:** an Open line of 29 September 2026 (#199) asked where both gates stand now that a value's type is read with `.type`. Answered that night (Q-0022): `.type` is gated as any `.` field is, and no gate stands at it today. Which read the fine gate stands at is the rule's Open line since.
- **Seed detail (27 September 2026):** (August 2026) the view registers ambient like every identity; the fail-closed target waits for the grant path (a capability passed by argument, needing `type`-typed parameters).

### A language is a section with a written start of five names
- **History:** six names on the morning of 4 September 2026 (`lex` among them); `lex` left that evening.

### A language opens closed; the door back, `logos (…)`, is opt-in
- **History:** that morning `{ }` was "the hole to the surroundings" of every closed scope; rolled back that evening.

### Scheduling, preemption and cancellation happen at boundaries
- **Seed detail (27 September 2026):** the bootstrap can ship cooperative `.await` yielding and add boundary preemption as the compiled tier matures.

### Case study: an RCU library's comment-only rules become machine-checked
- **History:** "on failure ownership stays with the caller": superseded 3 September 2026 (›An `own` argument is consumed at the call‹); in v0.1.0 a failed call is a fault.

## Mutability and construction

### Writing a value writes in place; the dyad keeps its address
- **History:** June 2026, `mut` was recast as a type modifier and phase bits were proposed as storage. Both are gone, see the History lines of the rules below.

### Left of `:=` stands one name (or regex), with its gate words in front, and nothing else
- **History:** 5 September 2026 the gates were prefix constructors on the right, `x := pub 5`, `y := pub mut i32 5`, binding just above `:=` so `x := pub a + b` gated the sum. Its ground, "a name is a placeholder for an identity, not a thing attributes attach to", was overtaken on 7 September when gates became entries on the name's binding. Replaced 15 September.

### A gate word reads the declaration to its right
- **Seed detail (27 September 2026):** the seed's `pub` has done this since August 2026; it never followed the 5 September respelling.

### `?` is one value, the unknown; `key := T ?` declares without a value
- **History:** before 23 September, `?` was a constructor building a fresh dyad with both slots `undefined` at every appearance, a literal, so `x := ?` bound x "to its own hole rather than aliasing one global unknown". Replaced because freshness belongs to the place.
- **Seed detail (27 September 2026):** a bare `?` stands as the one identity. `?` reads a type to its left only when that type applies to it, not when an operator waits there (`x != ?`). `T ?` still allocates zeroed bytes, which no read reaches.
- **History (28 September 2026):** `key := ?` questioned by Thobias ("i dont think x := ? should be possible to write though i dont see any reason why you wouldnt give it the type when its declared") and kept; the reason is at the rule. Seed detail the same day: the seed still makes the null-typed placeholder node at the lex and retypes it in place at the fill, the shape #166 would remove.
- **History:** until 29 September 2026 the 28 September ruling's reason went on: "`lhs := dyad ?` would hand the run body an unrun node, so `r * a` could not settle its operation once per node and the operand would run again inside the loop; the seed refuses `a * 2` on a `dyad ?` field." Superseded by ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹ and its rulings of the same day (Thobias, #199, "i want it to actually work"): a `dyad ?` is read through, so `a * 2` on one reads the node it holds and the seed's refusal is a lag, and a function over one is built per set of argument types where the call is parsed. An argument that must run leaves its value there, run once when the call runs (›Positional arguments fill the holes in order; an expression line among them is a precondition‹). The ruling, `key := ?` stays, is not reopened.

### Reading a place before its first write is refused at parse
- **Seed detail (27 September 2026):** the entry refuses at parse every use of the name except the target of `x = …` and the binding read `x:…`, until the sibling write.
- **Quotes:** Ruled: "maybe it should be a separate gate?".

### A value of several fields is filled one field at a time
- **Seed detail (27 September 2026):** narrowed 26 September 2026 (Thobias: "filling fields one by one is fine"): for a type whose values are nodes, `v := T ?` is a new empty node and writes to its fields fill it one by one, as a parse fills its node (see *An array expression is a node of `array`*). The general rule widens this to every type with fields.

### `:=` decides nothing about copy or reference; the type's reading rule does
- **Seed detail (27 September 2026):** has always copied data here.

### `:` is not a declaration operator
- **History:** `key : T` and bare `key :` were the old valueless forms; replaced 2 September 2026 by `key := T ?` and `key := ?`.
- **Seed detail (27 September 2026):** retired `:` with the driver convergence (September 2026, #59). It accepts `key := T ?`, `?` being an identity just below application that reads the type to its left.

### Parentheses are one identity, near the top of the order
- **History:** July 2026 wording: grouping paren loosest, call paren tightest. It described the superseded model; replaced 2 September 2026.

### A parse is ranked by what it takes from its right
- **History:** as ruled it said "a fields block that fills `shared parse`"; the fields block is gone (25 September 2026, *A type body describes one level*) and `shared` is spelled `share` (26 September 2026).
- **Seed detail (27 September 2026):** the numbers are already the seed's (application and an unfilled `share parse_rank` 91, `fn` 92). Two blockers were recorded open and closed 25 September 2026: a parse written in Logos running at discovery could not read past what is lexed (the #61 stand-in, closed by the lazy-read ruling of *The scope's constructor is the driver*), and the chooser reading `i32` would wake `i32`'s own read of the bracket (closed by *A cell a constructor reads from the tape arrives unbuilt*: the chooser gets `i32` unbuilt and `(1, 2, 3)` as a bracket; "just below it" means a bracket is built as it is lexed and a juxtaposition is not). As written on 25 September, array.logos still stopped at `tape[0] = get_mint(t)`, a cell read from the tape not being taken as a `type` argument.

### `mut` is a gate on the binding, not a type
- **History:** first recorded as "`mut T` is the type of a mutable T" (June 2026): mutability stated by the type, `&mut T` the same `mut`, per-field mutability on each field's type (`f := mut F ?`), and `mut` left-prefixing any level of the classifier tower, giving four construction states (`i32 32`, `mut i32 32`, `mut type i32 32` reinterpret-only, `mut type mut i32 32`). Also `mut key` for name-mutability and `mut (…code…)` for a structurally mutable region; these stratified as compilable value writes, deopting type writes, interpreted `mut (…)` regions. A 4 September conflict with `pub mut x := 5` was resolved 5 September by this rule. Before 15 September the gate was spelled `x := pub mut 5`.

### A writable pointer is made by `mut` on what makes the pointer
- **History:** the older spelling `&mut` (`&`'s own spelling) was respelled 20 September 2026.
- **Seed detail (27 September 2026):** `mut alloc` accepted since 25 September 2026; the pointer carries no mark yet, and a write through `@` is unchecked.

### One binding per declared name
- **History:** 5 September 2026 split a name entry (scope, liveness range) from a separate identity binding (lifetime, gates); replaced 7 September. The entry was once called `id_context`.

### A gate word stands only where a binding is being filled
- **History:** first wording, "an anonymous value is gated the same way because it has the record and never needed the name", replaced 7 September 2026.

### An access along a path needs every binding on the path to grant it
- **Seed detail (27 September 2026):** since 20 September 2026, `q.v = 3` needs `mut` on `q` and on `v`; `t.y = 4` needs it on the member alone. A type's own parse fills a field through the field's default entry (›Fields start writable by the type's own parse‹).

### Fields start writable by the type's own parse
- **History:** ruled over the fill `this.lhs = tape[-1]`; `this` was removed 26 September 2026 and a parse writes fields as `tape[0].f`.
- **Seed detail (27 September 2026):** both since 20 September 2026.
- **Quotes:** Ruled: "immut is the word".

### What still holds of the old lifecycle
- **History:**
  - Three-state lifecycle `undefined` → defined → frozen for both slots, with `mut` on the classifier keeping a slot writable past definition, and exclusivity making undefined-slot checks local ("the flip *is* publication"). Superseded 7 September 2026: there is no phase and no frozen state; writability is the gate on the binding.
  - Phase stored as tag bits in the low four bits of the 16-byte-aligned type pointer, not as wrapper nodes (a wrapper breaks type transparency); reflection could synthesize a wrapper view. Proposed June 2026, superseded 7 September 2026: `undefined` is a null slot and writability is a gate.
  - "The default gate of a declared name is write allowed while the slot is null" (7 September 2026). Removed 20 September 2026.

### `immut x` removes `mut`; `lock` seals the gate set
- **History:** the one-way phase flip `immut x` (June 2026): owner-only with no live borrows, like a move; frozen was final so a flip never invalidated anyone. Respelled 7 September 2026 into gates plus `lock`.

### Two muts, and the storage partition: `share` places a member once, unmarked members are per value
- **History:**
  - Derivation (July 2026): a field ever mutable, even only during `init`, is per instance; one never mutable is stored once with the type; placement decidable at the declaration site. Superseded 19 September 2026 by the `share` word (reasons above).
  - 19 September 2026: `share` was "placement, not a gate". Superseded 20 September 2026.
  - Members stood in a `fields = (…)` block (spelled `instance` until 19 September evening), and the type read them as `point.fields.dims` (23 September 2026: "point.dims is actually not allowed"). The block was removed 25 September 2026 (*A type body describes one level*): members stand in the type body, and `point.dims` reads through the type.
  - The mark was spelled `shared`; renamed `share` 26 September 2026, Thobias ("one letter less").
- **Seed detail (27 September 2026):** since 20 September 2026 the reader puts it on the member's binding, first in the set. Since 24 September 2026 (#141) a node's `.` finds a `share` member where no field of its own answers.

### `share` means one place, the same value at the same time, everywhere it is written
- **History:** 19 September 2026, `share` was a reserved word only inside a type's field block ("yes reserved inside instance inside type"), an ordinary name elsewhere. Widened 25 September 2026 to function bodies, then to everywhere.
- **Seed detail (27 September 2026):** since 25 September 2026 every position gets a global place, the initializer runs once as the definition parses, an owning value there is freed at program end, and the line itself then only names the place. A function whose `share` name holds a map is not compiled (the checked "cannot be compiled yet" every map in a compiled body gets).
- **Quotes:** Why: "maybe you want the same shared value across all imports?"; Ruled: "it can be used on names inside functions to get some persistent storage across runs of the same function"; Ruled: "shared should be available everywhere".

### Resolution is one rule
- **History:**
  - 30 August 2026: `a.x` probed the type's field scope, then `a`'s own bare scope; mut decided place vs stored dyad. 19 September: the mark decides.
  - 23 September 2026, Thobias: `point.dims` refused, `point.fields.dims` the read. Reason then: one word, `fields`, as the step from a type to what its instances hold. Superseded 25 September 2026 when the fields block was removed.
- **Seed detail (27 September 2026):** since 24 September 2026 (#141): a node's `.` finds a `share` member where no field of its own answers; `t.fields.x` reads one through the type and `t.x` is a checked error pointing at `t.fields.x`. (That was the 23 September spelling; the 25 September ruling makes `t.x` the read.)

### The ground is one identity, spelled `type`; `logos` is a language
- **History:**
  - July 2026: `logos` was the ground identity's proper name and the definition keyword, and "`type` and `struct` … never as separate core identities". Superseded 4 September 2026; sketches and examples respelled.
  - 18 September 2026, Thobias: the ground spelled `word` ("everything we write are words"; a definition gives a word its spelling, parse, run and what its nodes hold, more than a traditional type states). Objections recorded then as outweighed: the binding is the word level (in `^ := word (…)` the word is `^`), types with no word (`fn (…)`, `@i32`, `array i32`, an anonymous definition), data types that are not words (`point`), and DESIGN's prose using "word" for a spelling. Reverted 19 September 2026: the two-level split confirmed those objections. The dyad field stayed `.type` throughout ("dyad.type doesnt need to be changed to dyad.word").
  - Ground test was spelled `x:dyad.type == type`; `a:type` replaced `a:dyad.type` 23 September 2026.
- **Seed detail (27 September 2026):** spells the root `type` and keeps `logos` as a transitional alias of it until the `language` identity exists to give `logos (…)` its ruled meaning. #131 closed unneeded; the README spells `type`.

### `undefined`, `?`, `@void` and `native`
- **Seed detail (27 September 2026):** all primitives are `native`, ported to reflectable Logos source one identity at a time (*The architecture*).

### Two muts, and the storage partition
- ›Two muts, and the storage partition: `share` places a member once, unmarked members are per value‹ merged into ›Two muts, and the storage partition‹ on 27 September 2026
- ›`share` means one place, the same value at the same time, everywhere it is written‹ merged into ›Two muts, and the storage partition‹ on 27 September 2026
- ›The mark is spelled `share`, and slot fills carry it too‹ merged into ›Two muts, and the storage partition‹ on 27 September 2026
- ›A type stores its shared data once; what differs per value is stored per value‹ merged into ›Two muts, and the storage partition‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 20 September 2026 the reader puts `share` on the member's binding, first in the set; since 24 September 2026 (#141) a node's `.` finds a `share` member where no field of its own answers; since 25 September 2026 every position gets a global place, the initializer runs once as the definition parses, an owning value there is freed at program end, and the line itself then only names the place, and a function whose `share` name holds a map is not compiled (the checked "cannot be compiled yet" every map in a compiled body gets); the spelling `share` and the mark on slot fills not yet (#153), per DESIGN.

## Native interop and the Rust bridge

Nothing moved from this section.

## Error handling

### Target: errors are values (`T!`), handled by `match`, passed on by `try`
- **Seed detail (27 September 2026):** none of it in v0.1.0 (see next rules). It lands later as library and driver work over tagged unions; these shapes stay the target.

### Target: a constructor returns `void!`, so a user's syntax error is a recoverable value
- **History:** 30 August 2026: constructors "carry no `!`"; replaced 31 August.
- **Seed detail (27 September 2026):** v0.1.0 constructors are `-> void`; the resynchronizing driver comes later.

### Error values are in v0.1.0; an error names a category, which is a scoped name
- **History:** supersedes the 2 September 2026 staging-out below.
- **Quotes:** Why: "its rather just only a name without and type or value … in one scope you can define error.X and in another you can defined the same error name error.X and they are actually different … maybe a better word is error category".

### There is no bare `error «…»`; `abort «…»` stops the program; `alloc` returns `@T!`
- **Seed detail (27 September 2026):** `error «…»` is the fault today and becomes `abort «…»`; `alloc` is not `!` yet; both pending.

## Meta-reflection and the unified system

### Paradoxes are handled by stratification, only if needed
- **History:** qualified August 2026, once proofs became rewrite rules.

## Representation, elaboration, and lifetimes

### A plain number is a `rational_number`, and a number type's definition converts it (headed "Numeric literals are uncommitted until context classifies them" until 29 September 2026)
- **History:** until 29 September 2026 the rule was headed "Numeric literals are uncommitted until context classifies them" and read "It can become any `int`, `uint` or `float`. Beside a typed value it takes that type … the result stays `rational_number` until it lands in a typed slot." Reworded by Thobias's ruling (#199): "the working of "stays rational_number until lands in a typed slot" is wrong wording because rational_number is already a type. its just that i64 converts rational_number to i64, which should be part of the definition of i64." A number is not uncommitted; it is a `rational_number`, and the conversion belongs to the type it lands in.
- **Seed detail (27 September 2026):** folds all-literal arithmetic exactly over `i64` fractions; out of range is a clean error, not a wrap. Arbitrary precision deferred, not blocked.
- **History:** until 28 September 2026 the seed carried a run-time rational as the address of a literal node made per operation, a `dyad` view boxing a literal; the sixteen-byte value in its place superseded that (#165).

### No implicit coercion; a numeric type applied to a value is the conversion
- **Seed detail (27 September 2026):** `not` takes a bracketed operand, a parsing shortcut, not a keep.

### `-` before an operand is negation: one identity that reads its left side
- **History:** first worded "binds tighter than the binary arithmetic operators"; corrected 5 Sept, since one rank cannot do that.
- **Seed detail (27 September 2026):** rejecting `-y * 3` is a bug, folded into #59.

### Text literals are plain values; `#` is the one comment constructor
- **Seed detail (27 September 2026):** builds the comment node over `string` in both forms: `# raw text` to end of line, and `# «…»` bounded by the string (may span lines; anything after `»` is live code). `#` becomes a graph-resident constructor at self-hosting.

### The tape is Logic Graph with a string extension
- **History:** `token` as a wrapper identity superseded 2 Sept 2026: an unconstructed cell points at the identity itself.

### The driver lexes a segment whole and builds it highest `parse_rank` first
- **History:** replaced one-token-lookahead shift/reduce. "Operator precedence" gave way to the slot name `parse_rank` 10 Sept 2026.

### `parse_rank` is one `f64` on one shared axis, and may be relative
- **History:** written without `share` until 26 Sept 2026 (l.207: slot fills carry `share`).

### Expressions are self-delimiting; `,` stands between every two expressions of a scope
- **History:** before 26 Sept: "between independent expressions nothing at all stands, while between dependent ones the `,` is required", which disagreed with the seed's rule (›The scope's constructor is the driver‹).

### `if` needs no brackets around a complete condition or its body
- **History:** 2 Sept 2026 declined bare `if c body`. 5 Sept: any constructed cell may be a condition (`if` reads its own right side). 25 Sept: bodies decline reversed.
- **Quotes:** Ruled: "brackets should not be needed".

### `,` outranks a type's optional operand: `f(i32 3)` is one argument, `f(i32, 3)` two
- **History:** "`3` stays an uncommitted literal" until 29 September 2026, when a plain number became a `rational_number` from the start (›A plain number is a `rational_number`, and a number type's definition converts it‹, #199).

### `compile` never fails on an uncompiled Logos callee
- **Seed detail (27 September 2026):** since 10 Sept 2026 emits the jump into the interpreter, and a second compile (`f.compile()` in the seed's spelling today) lifts a boundary. Before, it failed fast and left the caller interpreted.

### Which machine operation an operator is (`+` over `i32` → `add_i32`) is a lowering rule
- **History:** before 8 Sept: "the parse-time constructor that resolves an application to one of them".
- **Seed detail (27 September 2026):** `+` resolves at parse, the divergence to remove. Run tier follows the rule since July 2026: every native is a `callable` value `[entry: @exec, convention]` (the type of `add_i32` and of compiled fn code alike) in the dyad's op slot; the interpreter consults no table. Cranelift lowering still uses a Rust-side table until lowering is re-keyed per backend identity.

### Execution is function application: to evaluate a dyad, read its type; run a `fn`, run a type's `run`, else it is data
- **History:** first: "Everything runnable is a `fn`", two branches only.

### A user-defined operator is a `type` that fills its own slots, `run` included
- **History:** 4 Sept: `code = fn (a := i32 ?, b := i32 ?)` over a positional operand record; "`constructor`, `destructor`, and `code` are three slots of one shape, each holding a function". 16 Sept: `run` holds a body (next rule). 17 Sept: renamed `run`; the `fn` type's member `run` gave up its name (running a node is the evaluation rule; `f.run` is the body it runs). 19 Sept: `shared run = (…)` in the fields block, bare `run = (…)` the type's own. 25 Sept (l.203): fields block gone, `^.run` not `^.fields.run`. 26 Sept (l.207): mark spelled `share`, required on every slot fill; an unmarked fill is the checked error.
- **Seed detail (27 September 2026):** the 4 Sept shape ran beside the new (#126) until #133 slice 9 deleted it, as ruled 20 Sept 2026: "to be deleted, not kept beside the new one, once the new shape runs".

### `run` is a bare body over the node's own fields
- **History:** fields declared in `fields = (…)` 16-25 Sept (l.203); read as `this.f` 17-26 Sept (l.207).

### A parse connects each operand to a field by name and removes the cells it took
- **History:** 16 Sept: `tape[0]:dyad.value.lhs = tape[-1]`, stamp `tape[0]:dyad.type = ^` (respelling ›The preview milestone‹'s 9 Sept "so that .operands is valid"). 17 Sept: `this` = shorthand for `tape[0]:dyad.value`; first write into `this` built the node (so a self-typed node needed no stamp: "the stamp said it twice"); `this` legal only in a constructor or `run` body inside `type (…)`, found by walking up the scope spine (#123); first use converted the cell ("this needs real lots of reflection on the LG"). 18 Sept, Thobias: `this` a separate fresh node placed by `tape[0] = this` (reason: placement written where it happens, no act on a first use), the write word.logos's own `parse` already made with `tape[0] = dyad (type, definition)`; it could be placed anywhere (`tape[1] = this`, `tape.insert(k, this)`) or nowhere (edit-and-leave error, #81). 19 Sept: in the instances' `parse`, `this` was the instance that appeared. 23 Sept: `:dyad` retired for `a:type` (Thobias had disliked writing `:dyad` first). 26 Sept: `this` removed (l.207). Declined 18 Sept: `value` as `this`'s name (it is the slot name in the same body, would name the block inside a `fields = (…)` line, and `tape[0] = value` places a bare value needing its type back); `node` noted as the only alternative.

### `tape` is a word the `parse` slot declares into its body
- **History:** replaced the `fn (tape := parsing_tape ?) -> void` wrapper. `this` was declared the same way 17-26 Sept (in `parse`, `run`, `drop`); a helper read it as an outer name under "a call is a use of every outer name" (#125).

### What `=` does is decided by the type of the place on its left
- **History:** also read `fields = ( … )` as the field declaration block until 25 Sept.

### A field is filled at run, per evaluation; its type decides how the operand is used
- **History:** an Open line from 16 to 28 September 2026: "the two moments of a field (operand slot written at parse, frame place evaluated at run) are implied but not yet said in one sentence." Closed by the two-moments sentence at the rule.
- **History:** until 29 September 2026 the two-moments sentence ended "for `dyad ?`, the node itself, unrun." Superseded by ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹ (Thobias, #199, "i want it to actually work"): a `dyad ?` place became a transparent placeholder, so what it holds is said at that rule: the operand's value, its graph staying in the node's slot.
- **History:** an Open line of 29 September 2026 (#199) asked which declared type keeps an operand as graph now that `dyad ?` holds the value. Closed that night by Thobias's choice of `@dyad ?` (Q-0022).

### A node's output type is per node, and its parse writes it
- **History:** 16 Sept: `output := type ?`, `tape[0]:dyad.value.output = tape[-1]:dyad.type`.
- **Quotes:** Ruled: "need to rename to output_type everywhere".

### A member stands before any body that reads it
- **History:** open 18-20 Sept as "must a `fields = (…)` block precede a body that reads `this.f`?"; power.logos already had that order.

### The constructor sets `tape.is_constructed[0] = true` itself; a tape write never sets it
- **Seed detail (27 September 2026):** since #133 slice 6: tape write replaces the pointer alone; `tape.is_constructed[k] = true` is a native; a Logos constructor is judged by the flag; a Rust constructor's decline sets it.

### `fn` is not a primitive: a function is a type in this same shape
- **History:** the `fn` type was two shared functions, `compile` (lower body to `bcode`) and `run` (execute), plus per-instance `input`, `output_type`, `body` (reflectable graph), `bcode` (opaque, null until compiled), `frame` (byte size of parameters and locals per call, null when none).
- **Seed detail (27 September 2026):** #127, after #126.

### The executing primitive has two paths: jump to `bcode`, or walk the body
- **History:** was the `fn` type's member `run`; lost that name 17 Sept 2026.

### A function's surface is `fn (params) -> T (body)`, and its parameter list is a record type
- **History:** a colon field form replaced 2 September 2026.
- **History:** an Open line of the morning of 29 September 2026 (#199): whether `fn (a) -> i64 ( a )` is refused where it is defined, a `dyad ?` holding "the node itself, unrun", or runs with `a` holding a value whose type is checked when the body runs. Closed the same day: it works ("i want it to actually work"), through ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹.

### Positional arguments fill the holes in order; an expression line among them is a precondition
- **History:** for some hours on 29 September 2026 the #199 Ruled line said "`a:start` reaches the graph where `a` is declared and defined, the nodes `a`, `:=`, `x`, `+`, `1`", from Thobias's "if you do a:start you get to the actuall LG where a is declared and defined". Narrowed for a parameter the same night by his choice of option (a) (Q-0024): a body is built once per set of argument types and `a:start` is fixed at the build (›A binding's fields are read at elaboration, at the line of the read; running code never reads a binding‹), so one `a:start` cannot show each call's argument. A parameter's `a:start` is where `a` is written in `fn (a)`, and the argument's graph is reached from the call.

### Mandatory named arguments are rejected
- **History:** a third reason (the callee is asleep while `(` builds, so `x = X` hits an undeclared name) was superseded 3 Sept 2026: a callable reads its own bracket (›The scope's constructor is the driver‹).

### A type stores its shared data once; what differs per value is stored per value
- **History:** July 2026: `shared` was a placeholder, not real syntax; placement was to be *derived* from mutability (a never-`mut` member is the same in every value, so stored once), and `shared` would disappear once `mut` and `pub` landed, reflection reading "shared" off "immutable". Changed August 2026 (l.201): since a per-value constructor is immutable yet differs per value, placement is written structure and can no longer be derived from mutability. Changed 19 September 2026: `shared` is real syntax, the one mark of placement; reflection reads the mark. Respelled `share` 26 September 2026 (l.207).
- **History:** 18 September 2026: bringing back `shared` for a member all values share and may write was proposed and withdrawn, because the case behind it (a generic's memo of its mintings) was a plain variable, not value data. 19 September: shared-and-mutable state became a `shared mut` member of the fields block, the memo staying a plain variable beside its chooser. 26 September (l.205): the memo is `array`'s own `share` member.

### The constructors drive the tape
- **History:** a schedule byte stood in until the constructors drove the tape; retired July 2026. The seed's NaN / finite / +infinity three-way classification of `parse_rank` was superseded 30 August 2026 by "every identity has a `parse_rank`".
- **Seed detail (27 September 2026):** stores the metadata head in full as each identity's metadata head (not on the name's binding of *Name resolution*). The core `parse` is a `native` callable leaf (called, never read into, per the reflection boundary) under one entry signature that self-hosting will replace with Logos source.

### Slots are named for the moment they run: `parse`, `run`, `free`
- **History:** `drop` until 28 September 2026 (›`move` is the act, `own` the gate word, `free` the end‹).
- **History:** the slots were `constructor` and `destructor` until 17 September 2026. The 17 September text also said the building of an instance was `this`'s job; `this` is gone since 26 September 2026 (see *There is no `this`*).
- **Seed detail (27 September 2026):** #130 renamed the slots, README with it.

### `parse_rank` and `associativity` are data, and the only triggers
- **History:** whether the one global number should become a partial order of binding groups was open, closed 30 August 2026.

### `lex_rank` belongs to the binding, not the type
- **History:** `lex_rank` was listed among the slots `type` declares until 14 September 2026. The open question "how a definition writes `lex_rank` and a gate from inside the right side of `:=`" was closed for gates 15 September 2026: a gate stands left of `:=` beside its name (*Declarations are immutable by default*).
- **Seed detail (27 September 2026):** since 14 September 2026 (#122): `x:lex_rank` reads, `x:lex_rank = …` writes, the lexer ranks every match by its binding, the two fresh patterns carry theirs from the start, and the head's slot is gone. Stand-in: a `lex_rank = …` line in a `type (…)` body writes the binding of the declaration the body is the value of; the checked error where no declaration is being filled. Kept because a pattern identity has no other spelling to be written on; to be deleted once it has.

### A slot is filled with `=`; `:=` declares a member of your own
- **History:** a further reason was given for the fields block: `^.parse_rank` probed the `fields = (…)` scope before `^`'s own bare members, so a redeclared member would be found second or never (block gone 25 September 2026). `:=` spellings of these slots in earlier sketches were respelled. `fields` (spelled `value` 17 to 19 September, `instance` 19 to 23 September) was a slot too, until 25 September 2026. l.201 wrote the fills without the mark (`parse_rank = …`).

### Slot words are known only inside a type body
- **History:** earlier 19 September 2026: the slot names were the core words, known everywhere, `=` with one on its left filled the innermost definition's slot, and outside any definition it was the checked error. Superseded the same day.
- **Seed detail (27 September 2026):** the root identities of #133 slice 4 were undone 20 September 2026: a type body declares the six slot words into a scope of its own around the body, closed with it.

### `free` is one word
- **History:** ›`drop` is one word‹ until 28 September 2026.
- **History:** 17 September 2026: a bare `drop = ( … )` in a type body named the slot, the fields-block binding being "the innermost `drop`" the lookup meets, never the `drop x` statement (#130 ambiguity). Superseded 19 September 2026: there is no second `drop` to be innermost.
- **Seed detail (27 September 2026):** `=`'s constructor, not `drop`'s, takes a lone `drop` to its left as the slot's name, since `=` constructs at discovery before a prefix keyword's turn (divergence). #130 closed into #132.

### Fields are read with a dot outside their scope
- **History:** l.201 (30 August 2026) had "a field's declaration is checked only against its siblings"; superseded.

### Sketches for type bodies
- **Seed detail (27 September 2026):** `run` is installed on the type as the body's lexed cells, with the functions constructed from them per field-type set kept beside them (since #133 slice 8; the `fn` form deleted since slice 9). A `share` member is read through the type alone, `g.y`, not `inst.y` (#116); Thobias, 23 September: "the shared fields are also readable from per instance its just that its the same across all instances".

### A type body describes one level
- **History:** see *History of type-body layout* at the end of this part.
- **Seed detail (27 September 2026):** since 26 September 2026 (#150): a type body is read as one list of fields, share members, slot fills and prose; a type stores one parse, rank and associativity; a value that appears wakes its type's parse; a node a Logos parse placed for its own type is not woken again; `^.run` and `point.dims` read through the type; the fields-block errors are gone. Prose before a field or member is read past; before a slot fill it is kept beside it. (#150 made a bare slot fill legal and `shared parse = (…)` the checked error; that rule is superseded by the `share` ruling, see below.)

### A value never has its own parse; the only parse is written in a type body
- **History:** see *History of type-body layout*.

### Elements are built by assignment
- **History:** the tape affordances `tape.construct(T, v)` and `tape[k].dyads[i] = v` are deleted 25 September 2026, since only the array used them. The example was written `(this.ptr + i)@` until `this` went (26 September).

### An array expression is a node of `array`, and its run makes the value
- **History:** the name `fill` is gone, its body being `array`'s run ("dont think fill name is needed at all", Thobias). This supersedes, in *The placed call returns the mint*, the placed call `this.fill(tape[1])` and its per-run copy of `this`; the run now does that per-run work.
- **Seed detail (27 September 2026):** since 26 September 2026 (#151): `array T [ … ]` is a node of `array` whose run makes a new value each time. `array`'s parse leaves the mint when no bracket follows. `v := T ?` of a type whose values are nodes is a new empty node. An untyped field holding a bracket has its lines evaluated at each run where the node stands. A node a Logos parse placed, of any type, is not woken again by its own type, while the value its run yields wakes the value's type, so `array i32 [4, 5][1]` reads 5. Stand-in for #152: a square bracket taken by `=` into a place of a mint (`x = [4, 5]` and each line of a nested list) is built in Rust as an `array` node over it; #152 recognises a mint as a type minted while `array`'s parse runs and reads its element type from its `element_type` member.
- **Quotes:** Why: "its nice to have it there for reflection".

### The memo and the minting are `array`'s own members
- **History:** supersedes *The chooser's mintings live in a plain variable* (its reason).

### There is no `this`: a type's bodies name fields directly; a parse builds its node in `tape[0]`
- **History:** superseded 26 September 2026: `this` legal only inside a constructor or `run` body (17 September); `tape[0] = this` as the placement (18 September); "this should be used inside instance as well" (19 September, a body reached through an instance bound `this` to it, and in the type's own parse `this` was the fresh instance); every `this.f` spelling in DESIGN, the sketches and the seed. The stamp was first written `tape[0].type = array` (August). Kept: the per-run copy of a parse's fresh node (*The placed call returns the mint*) for a call a parse places on the node it is building.
- **Quotes:** Why: "just simply write the fields directly in tape[0]"; Why: "logos already uses non shadowing though so people should get used to creating expressive names to avoid name conflicts … remember logos uses non shadowing like zig"; Why: "what about just writing tape[0]:type = array ? that should initialize it. if something tries to read or write before that … it should be an error unless the read or write matches the type tape[0] already has".

### The mark is spelled `share`, and slot fills carry it too
- **History:** supersedes the 26 September seed rule of *A type body describes one level* (a slot filled bare, `shared parse = (…)` the checked error). 19 September 2026: `shared` on the fill was one place, an unmarked fill a per-instance default, an unfilled slot a place per instance costing nothing until written, the layout locking at the first instance (reason then: per-instance behaviour takes the same mark as per-instance data).
- **Quotes:** Why: "share is fine".

### History of type-body layout
- **Superseded record:**
  - **August 2026, parse dispatches through the type:** an identity's `parse` ran for appearances of its *instances*, so the code elaborating `array i32 0` lived on array's type ("the C++ intuition restored one honest meta-level up"); shared behaviour lived on shared types (operator, keyword, data type; the record's kind byte lifting into real intermediate types); unique syntax lived on a singleton type per identity. Realizing it was #48's mandate. Superseded the same month by the constructor-as-field ruling (the dispatch reading and the singleton shape; its ground kept: fail-closed inertness, no climbing, parse behaviour readable as data).
  - **August 2026, the constructor is a field:** `parse` was an ordinary field a type declares for its instances, and where it was declared decided everything: per-instance (each instance supplies its own; `type` declares it so, which let each type and keyword define its own parse in its own definition, `array := type ( parse = (…), … )`, an `if` or `import` a one-liner, deleting the metaclass tower), shared (one behaviour for every instance), or absent (inert). Dispatch: an appearance of X ran X's `parse` field. Superseded 25 September 2026 by *A value never has its own parse*; the deletion of the metaclass tower stands.
  - **August / 30 August 2026, two directions in one body:** bare body lines belonged to the identity itself (its slots, and new members living on it, a namespace's); everything about its instances lived in an inner block. 19 September 2026: a bare `:=` line in a type body became the checked error; every member went in the block, shared ones marked. Superseded 25 September 2026: one level, members and slot fills stand in the body together.
  - **The block's name:** `instance` (disliked, 2 September 2026) → `value = (…)` (17 September: it names the dyad's `.value` slot) → `instance = (…)` (19 September, Thobias: the block carried behaviour too, so `value` named too little) → `fields = (…)` (23 September, Thobias, "i like fields more": the block held fields and nothing else). Gone 25 September 2026. Seed #128 tracked the respellings.
  - **19 September 2026, the block holds instances' data and behaviour:** every block started with predeclared slots (`parse_rank`, `associativity`, `parse`, `run`, `drop`, `instance`) filled with `shared … = (…)`; a bare `run = (…)` or `drop = (…)` filled the type's own slot. Reason then: `run` and `drop` are about the instances as much as their fields. Closed the 16 September open "where `run` is stored", the 18 September open on `parse := parse (…)`, and the #130 spelling. The `run` fill was held as its lexed tape until a node's field types are known (*Deferral is authored*, 20 September 2026). Superseded 25 September 2026.
  - **23 September 2026, `run` not a slot of the type itself:** "remove type.run. run should only be inside instance" (Thobias); the values' body was read `^.fields.run`, `^.run` the checked error; shared members read `g.fields.y`. Reason then: nothing wanted the type's own run, and it stood as an empty slot in front of the shared fill. Seed #141 (24 September) read `^.fields.run` through a `dyad` view. Superseded 25 September 2026: `^.run` and `point.dims`, `run` again a slot of the type holding its values' body.
  - **A type body as a plain scope (August):** `g := type (y := 3, z := y + 3)` saw `y` in `z`'s line, members reached `g.y`. 19 September: spelled `g := type (fields = (shared y := 3, shared z := y + 3))`. Since 25 September members stand in the body again (spelled with `share` since 26 September).
  - **Source:** DESIGN.md l.199, l.201, l.203

### Plain substrate words: dyad, `.type`, `.value`
- **History:** the view read was first written `(dyad a).type`, superseded 8 September 2026 by `a:dyad.type`, which `a:type` replaced (l.213). `logos` first survived in three places (the ground identity's name, the definition keyword, the language's name); superseded 4 September 2026: the first two are `type`'s (*Substrate vocabulary*).
- **History:** "The view read is `a:type`" until 29 September 2026, when a value's type became `a.type` (#199).

### Text is the quote: `lex «…»` hands back the tape, unconstructed
- **History:** the founding example was respelled 2 September 2026, `:` having left the declaration surface. l.209 writes the field read `if.constructor`; the slot is `parse` since 17 September 2026.

### What `lex` returns: a `parsing_tape` fragment
- **History:** 2 September 2026: the fragment was an `array @dyad`. Superseded 14 September 2026 by `parsing_tape`.
- **History:** until 29 September 2026: "Text that names nothing lexes to a fresh dyad with both slots `undefined`". The fresh dyad went with the one-word node (#166, ›An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value‹); the sentence followed on 29 September 2026 (review round 2 of #166).

### `lex` runs whenever it runs: no comptime/runtime split
- **Seed detail (27 September 2026):** since 15 September 2026 (#62): an identity at the rank of the raw-text consumers that reads its quote at discovery (as `regex` does) and lexes it when its node *runs*, against the scopes open at that moment (the appearance's, for a constructor: the seed's reading of "the lex site"). The runtime carries the lexer only while the parser runs something. Each run lexes afresh, so an undeclared spelling gets its own fresh dyad per run. The spelling rides on the cell, so a spliced cell keeps its text and the number pattern's constructor builds `2` from `lex «* 2»`. `insert` takes a tape; the single-cell insert is gone. Stand-ins left: `lex` reads only its quote (a string value has no storage to read yet); a `tape[k]` read past the frontier at run time stays the checked error; a spliced cell's error caret falls where the fragment's offset lands in the source, the derived source map being unbuilt.

### The tape is data with four affordances
- **Seed detail (27 September 2026):** the seed also has `tape.recenter(k)` as a native (see Seed status).

### A cell is a pointer to the binding the trie resolved
- **History:** `lex «array»` yielded an `array @dyad` until 14 September 2026 (see *Text is the quote*). 7 September 2026: a resolved cell was read as `tape[1]:scope`, `tape[1]:gate`, and its dyad as `tape[1]:dyad`; a constructed cell answered `:dyad` (its own dyad) and `:scope` (the tape's scope), under the path rule of *Meta-navigation* (8 September 2026). The `:dyad` read was later retired for `a:type` (see *The dyad's read surface*).

### `tape.spelling[k]` is the text a cell was lexed from
- **Seed detail (27 September 2026):** #121, 14 September 2026 (the seed's span already kept the text readable).

### The tape is a cheap view with a movable center, over a doubly linked list
- **Seed detail (27 September 2026):** `ParsingTape` is an arena of linked nodes, the center a node.

### Unknown spellings are two pattern identities
- **History:** 9 September 2026 (superseded 10 September): a fresh spelling reached from where the trie stops matching to the next whitespace, bracket, `,`, or start of a known spelling. So `^ := …` declared `^`, `a:=1` lexed `a` then `:=`, `x^2` with `x` declared lexed `x`, fresh `^`, `2`; a first appearance wanted spaces. Reason then: the lexer must bound an unknown spelling before it sees the `:=`; whitespace, brackets and `,` are the only structure; stopping at a known spelling keeps `a:=1` and `f(x)` readable.
- **Seed detail (27 September 2026):** the letters-and-digits identifier scanner and the word-boundary check are divergences (#110).
- **History:** until 29 September 2026: "Each builds the fresh null-slotted dyad." The fresh dyad went with the one-word node (#166, ›An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value‹); the sentence followed on 29 September 2026 (review round 2 of #166).

### Every spelling has a `lex_rank`; rank first, then length
- **History:** first form, 10 September 2026, same day: only patterns ranked, and a literal met a pattern by length. First wording also claimed the boundary rule fell out of longest match; corrected the same day (under rank-before-length it does not).

### `[` is `(` in square brackets, and lands a `square_brackets` cell
- **History:** `tape[1].index`, first written in identities/array.logos, respelled `tape[1].dyads[0]` (25 September 2026).
- **Seed detail (27 September 2026):** since 25 September 2026 (#137), `[` lands a `square_brackets` cell whose `dyads` hold its lines. The seed's `[` still folds `t[k]` in its own constructor, a tape value having no constructor of its own yet.
- **Quotes:** Why: "square_brackets should be an identity just like scope is an identity".

### `a[k]` is an application, exactly as `a(k)`
- **History:** the first array.logos read the parse's own locals after the parse had returned, `tape[0] = scope ( … index … this … )`; superseded by this ruling. The 25 September spelling was `tape[0] = this.at(tape[1].dyads[0])` with `shared at := fn (index := u64 ?) -> element_type ( if index >= this.size error «…», (this.ptr + index)@ )`; since 26 September 2026 there is no `this` and the mark is `share` (a value in the cell is `tape[0]`, `tape[0].at(k)`; fields named bare in the function body, see l.207).
- **History:** until 29 September 2026 the rule read "A node's operands are the collection its type defines, `(x + x).operands[0]`". Superseded by Thobias (#199): "(x+x).operands should not exist. it should be lhs and rhs for all the other operators as well."

### A placed call keeps its tape lines as operands
- **History:** Claude's reading of the same day, "the placed call's arguments run as it is placed … so the node holds values", superseded.

### An index may be of any integer type
- **History:** before, the check `⊆ size:type` was the whole rule, and `k := u8 1, a[k]` was "these types do not match".
- **Seed detail (27 September 2026):** since 25 September 2026, in array.logos and in the placed call's read of a narrowed line.

### A call that ends in a dereference is a place
- **Seed detail (27 September 2026):** since 25 September 2026, `=` over such a call builds a copy of `f` whose body ends in `p` and returns `@T`, and stores through that copy's call; no array-specific code. A fault a compiled caller parks is the first one, so the null guard over the zero an interpreted callee's error left does not replace that error.

### A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type
- **History:** the 25 September wording said "a type whose fields block fills the instances' `parse`" and "`this` there is that name's value"; since 25/26 September a type has one parse and no fields block (l.203) and there is no `this` (l.207).
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): names, parameters, call results and brackets wake, interpreted and compiled; `a[1]`, `b := a, b[1]`, and `f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)` work, the last also after `f.compile()`. A field read wakes too (later on 25 September; before, `this.items[1]` over a field was refused).

### A list's elements are filled when the code runs
- **History:** the mint's fill loop read each line's number at parse, which refused a name among them; superseded 25 September 2026.

### A bracket goes to the call whole
- **History:** 25 September: the mint's parse placed `tape[0] = this.fill(tape[1])` with `fill := fn (elements) -> this:type (…)`. Since 26 September 2026 (l.205) `fill` is gone: `array T [ … ]` is a node of `array` whose `elements` field holds the bracket and whose run does the work.
- **History:** 30 August to 28 September 2026 the sentence read "At top level, where lines run as they are parsed": the eager top level, which ›The pass runs only as far as it must‹ superseded on 13 September without this sentence being updated. Reworded 28 September 2026.

### The array's list is written in square brackets
- **History:** `t := array i32, x := t [4, 5]` was also valid on 25 September; since 26 September a mint is not written before a bracket but assigned one, `x = [4, 5]` (l.205).
- **Seed detail (27 September 2026):** identities/array.logos since 25 September 2026, no Rust change.

### A field of such a type holds the node's address
- **Seed detail (27 September 2026):** since 25 September 2026, no array-specific code. A function holding the field read stays interpreted, as every field read does (only the per-run copy lowers); a compiled callee handed `b.items` runs compiled.

### Each evaluation of an array expression makes a new array
- **History:** 25 September: "the placed call returns the mint": the call yielded the mint as a pointer; Claude's spelling (open to Thobias) named the mint `this:type` inside its own fields block, and a call a type's parse placed on its `this` took a per-run copy of that node, begun as the parse left it. Superseded 26 September 2026 (l.205): the array expression is a node of `array` whose run makes the value; `fill` and its per-run copy are gone. Before the sealed read, the mint lexed past the return type into the body, and a function returning an array had to name the type first, `t := array i32, … -> t ( … )`.
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): `array i32 [x, y, 3]` fills at run; a function that makes an array compiles (the bracket's number lines run in the compiled frame; the new bracket and the per-run copy, nodes of the store, are made by a call from machine code back into the seed; the builder runs interpreted as any uncompiled callee does). A return type spelled `-> array i32 ( … )` stops before the body's bracket (the mint's `tape[1]` reads `void`), so `mk := fn () -> array i32 ( array i32 [1, 2] )` returns the array.

### `alloc n of T`, for such a `T`, gives n cells of its address
- **Seed detail (27 September 2026):** 25 September 2026, one test in `read.rs` for which cells load whole (`cell_numtype`), no array-specific code.

### A list's lines move into the value built from it
- **History:** `own array i32 ?` as a field was found (Claude, 25 September) not to be this mechanism, missing three pieces (`own` over a non-pointer hole, `drop` over a field read, a field waking its type's parse); closed later on 25 September by *A field may be `own t ?`* and *A field of such a type holds the node's address*.
- **Seed detail (27 September 2026):** the refusal stands where a type's parse hands a bracket to the call it places, for every type written in Logos; a callee that only reads its list is refused alike.

### The element type decides how a line becomes an element
- **History:** 25 September, Claude's spelling (open to Thobias): two tape affordances, `tape.construct(T, v)` (construct the two cells `T v` as the driver does a segment, give the one node, else the checked error) and `tape[k].dyads[i] = v` (replace line i of bracket cell k); array.logos wrote `if tape[1].dyads[i]:type == square_brackets or tape[1].dyads[i]:type == scope ( tape[1].dyads[i] = tape.construct(element_type, tape[1].dyads[i]) )`. Reasons then: the construct is the driver's own act ("the driver wakes by the flag and the cell's `parse_rank`", 19 September), offered to every Logos type; the line write was the 20 September "mutate the dyad array inside scope". Both deleted later on 25 September 2026 (l.203): elements are built by assignment, `(ptr + i)@ = elements.dyads[i]`, and `=` constructs each by the element type's parse.

### At top level, a `[…]` line of a list is not run early
- **Seed detail (27 September 2026):** 25 September 2026, the top level's pending run skips a `[…]` line.

### `{ }` is text interpolation
- **History:** extended to closed language blocks on 4 September 2026, rolled back the same day: a language block's door is `logos (…)` (see *Sections, the arche, and effect identities*).

### Every identity has a `parse_rank`, and `(` works in two steps
- **History:** superseded 30 August 2026: the incremental shift/reduce-with-holding form first recorded here; the one-token lookahead of *Elaboration*; the threefold applied/holding/declined answer; the NaN/finite/+infinity classification of *A type's metadata*; the zero-or-one-cell wording.
- **Seed detail (27 September 2026):** converged September 2026 (#59): the loop lives in the scope's constructor, every identity has a finite `parse_rank`, a leftover cell is the error, the `,` is required.
- **History:** 30 August to 28 September 2026 step 2 ended "At top level it runs them": the eager top level, superseded 13 September by ›The pass runs only as far as it must‹ without this sentence being updated. Reworded 28 September 2026.

### A constructor's outcome is read off its own cell; no holding, no re-invocation
- **Seed detail (27 September 2026):** #81, 15 September 2026: the outcome is read off the construct's cell by handle, whatever the center is after the call. Built, removed, or rewritten to another token is progress; an untouched frontier is the decline; a cell left unconstructed after an edit is the checked error. Nothing runs twice.

### An identity that changes what names mean inside its bracket, or when its body runs, reads its own bracket
- **Seed detail (27 September 2026):** `fn`, `for`, `while` parse their own bodies with the barrier pushed first; this is the ruled shape.

### `if` reads its own right side
- **History:** 3 September: "`if` keeps the plain order, its bodies being inline, once, and naming nothing new"; superseded 5 September.

### An `if` body needs no brackets
- **History:** before, bodies stayed scope cells, `if (c) (body)`, while a condition could be any built cell (5 September 2026).
- **Seed detail (27 September 2026):** 25 September 2026 (#137): a bare body is a scope holding one expression, ending at a `,`, a closer, or an `else`. An untaken bare branch is parsed and dropped, since only building it finds its end; the unlexed skip holds for a bracket only.

### A call stays a call in a condition, and `while` reads as `if`
- **History:** earlier the same day, in the seed: a condition ended at a value, `?`, or an identity right of a comparison, so `if 1 == f(x) (…)` compared with `f` itself and the call had to be written `if 1 == (f(x)) (…)`; `while` kept its bracketed body. Both superseded the same day.

### Anything yielding a `bool` is a condition
- **Seed detail (27 September 2026):** 25 September 2026 (#137): a value with a constructor (a fn value or an instance) takes a `(` after it as its call. A type left of a `(` is the compared value when it stands right of an infix its record ranks among the comparisons (below the range, above `not`), and otherwise reads its bracket, as `i32` does in `x + i32 (1) == 2`. An infix is what its record says, its first operand or field named `lhs`. `if` and `while` take any node whose function declares a `bool` output. The rank is a stand-in: whether an operator gives `bool` is known only once its constructor has built the node (a result fixed on the type is superseded, 16 September 2026, *Execution is function application*), and building it early to look, then again, is the re-invocation the driver rules out. So a `bool` infix ranked outside the comparisons, with a type right of it, leaves the `(` to the type; that shape is written `if (t ≈ i32) (…)`.

### Inside a right-side read, a `(` after a cell that is not yet an operand belongs to that cell
- **History:** "a bracket wanted inside a condition after its first cell is written by parenthesizing the whole condition": for `if`, narrowed 25 September 2026 by *An `if` body needs no brackets* (a `(` right after a complete condition is the body even where the cell before it could take it).

### `for a..b` with no index is the same loop without the variable
- **Seed detail (27 September 2026):** #129, in the seed since #133 slice 7.

### `X (…)` is one spelling, and X's constructor decides what the bracket is
- **History:** a type's constructor also took declaration lines as new shared members, per-instance fields locking at first instantiation (*Two muts*). Struck for `type (…)` 19 September 2026: a bare declaration line in a type body was not allowed, members being declared in the fields block. (The fields block itself is gone since 25 September 2026, l.203.)

### A scope's lines are its one field, `dyads`
- **History:** spelled `self` from the 2 September sketch until 24 September 2026, no reason recorded.

### Reading a path runs nothing
- **History:** until 29 September 2026 the rule's example ended "`(x + 2).lhs` reads `x`, as `.operands[0]` does". `.operands` went with Thobias's ruling (#199): "(x+x).operands should not exist. it should be lhs and rhs for all the other operators as well."

### `=` sits beside `:=`, and returns nothing
- **History:** until 28 September 2026 the rule said nothing of a displaced value, and the seed leaks it (#170).
- **Seed detail (27 September 2026):** 9 September 2026 (#60): `:`, `.`, `@` build at discovery with their right cell lexed on demand; a reader followed by a tight read is put to sleep before it drives; the seed's right-side drives lex onto a fresh tape; `=` drives at discovery. Application and juxtaposition also build at discovery, reading their bracket or literal lazily in source order (seed rank 91, above `(`'s 90), since a tight read above them would otherwise take the bracket first: `f(2).x` and `dyad (i32, 7):dyad` read the call.

### A cell a constructor reads from the tape arrives unbuilt
- **History:** 9 September 2026 seed rule: a cell lexed on demand was built at discovery when it read nothing to its left. So `i32` was built as it was lexed, took the bracket as a conversion, and failed with "a conversion takes exactly one numeric value".

### A reader's read builds an equal right-associative cell first
- **Seed detail (27 September 2026):** 25 September 2026, in the driver's lazy read, no array-specific code.

### A `tape[k]` read past the lexed frontier lexes on demand, for every constructor
- **History:** from #61 (9 September) until 25 September, such a read by a Logos constructor was the checked error, because it re-entered the driver from inside a constructor; a Logos constructor at or above `(` could not lex past the frontier.

### A read where the tape reaches no cell gives a `void` node, already constructed
- **History:** after the lazy-read ruling, such a read was the checked error "this index is off the tape". #147's second part (a tape index failing as an error value under `T!`/`try`) has nothing left to do for reads; it stays staged after v0.1.0 for a write past the reach only.
- **Seed detail (27 September 2026):** 25 September 2026 (#147). A tape read to its end point is sealed, so a later read past it gives `void`.

### Seed stand-ins that remain
- **History:** l.211 described a `type (…)` body with four slots filled by `=` and a `fields = (…)` block (spelled `instance` in the seed until #128) taking holes and bare names only, stored-once members there waiting. The fields block is gone since 25 September 2026 (l.203; seed #150).

### The dyad's read surface: two fields, and the type answers the rest
- **History:** until 29 September 2026 the rule said "The fields exist only where `s` is a dyad", gave as its Why "A value's type is never one of its own fields, so a universal `.type` on every value would be `.` doing a second job", and listed as Rejected "The universal `.type` metaproperty on every value: retired." Superseded by Thobias (#199): `.type` is read on every value, as a field `type`'s own body declares, because "type lives in the same payload as the value now" (the one-block node, #166). `.value` still exists only on a dyad.
- **History:** until 29 September 2026 the rule read "an operator node's slots are the fields its type defines, so `(x + x).operands[0]` reads the first operand just as `p.x` reads a record field", and gave "an operand index past the arity" as a checked error. Superseded by Thobias (#199, Q-0022): "(x+x).operands should not exist. it should be lhs and rhs for all the other operators as well." The example is now a node reached by path, `b:start.rhs.lhs`.
- **Seed detail (27 September 2026):** since August 2026 (#52 in part): `.operands[i]` is ordinary field access on any operand-kinded value; the type members (`.arity`, `.roles[i]`, `.parse_rank`, `.associativity`, `.parse`, `.drop`, `.fields`, `.size_bytes`, `.scope`) read the shared record as fields of the type value; every read folds at parse (comptime reflection, the regime a Logos-written constructor runs in); a mismatched read is the checked error.

### `a.type` reads the type of the value a name stands for (spelled `a:type` from 23 to 29 September 2026)
- **History:** until 29 September 2026 the rule was headed "`a:type` reads the type of the value a name stands for" and read: "`x:type == i32`, `i32:type == type`, `a:type is number` in a premise, `tape[-1]:type` inside a constructor, `b:start.rhs:type` on a node reached by path. … `:type` is the one `:` read that reaches the dyad level. The binding gate stands at `:type` (see *Sections, the arche, and effect identities*). `a.type` is an ordinary field read on `a`'s value, present only if its type declares a `type` field." Its Why: "The type is the one fact about a value that `.` cannot give (it is never one of the value's own fields), so it takes the short spelling. The cell was only ever reached to get its two slots: one is now `:type`, the other is `.`." Superseded by Thobias's ruling of 29 September 2026 (#199): the type lives in the same payload as the value since the one-block node (#166), so "type should always be accessed by .type instead of :type". Where the reflection gate stood at `:type` is asked at ›Reflection (`:type`) is fail-closed and gated twice‹. An Open line of the same day asked what `:type` of an expression nobody named gives: what it yields, or the node's own type word. His answer ruled the spelling and chose neither, so the question stands at the rule's Open line as what `.type` of such an expression gives, beside the tape operand's case.
- **History:** that Open line, on what `.type` of an expression nobody named gives in code or as a tape operand, was closed the same night by Thobias's leaning towards what it yields (Q-0022). How a node's own type word is read is the rule's Open line since.
- **History:** August 2026: the dyad view was `(dyad a).type`, replacing the universal `.type` metaproperty. 7-8 September 2026: respelled `a:dyad.type` (`:dyad` = the view; `(dyad a)` as a second spelling dropped). In that period `a:type` was a checked error. 23 September 2026: `:dyad` and `:value` retired, `a:type` replaces `a:dyad.type`.
- **Seed detail (27 September 2026):** since 24 September 2026 (#139): `:dyad` and `:value` are checked errors; the binding's pointer to its dyad stays a layout field that `:` does not spell.
- **Quotes:** Amended: "its just a simplification in the definition of : where it emmits dyad when accessing type and value but you can still write a:dyad.type".

### Writing a cell's fields is decided by gates
- **History:** 31 August 2026: a constructor could write `tape[0].type` and `tape[0].value` while its own cell was unconstructed, and "the flip to constructed seals both against the view". Superseded 7 September 2026 by the gate ruling of *Mutability and construction*.

### Inclusion between types is `⊆`
- **History:** array.logos first wrote `index:dyad.type ⊆ this.size:dyad.type` (old `:dyad` and `this` spellings).
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): `⊆` between two types, folded at parse when both are known, run interpreted when one is read at run (a tape cell's `:type`). A type includes itself. Two integer types compare by value range: `u8 ⊆ u16` and `u8 ⊆ i16` true, `i8 ⊆ u64` false. Any other pair (float and integer, two floats, `bool`, a record, `type`, `dyad`) is a checked error, what those types hold not being settled here.

### `.size_bytes` is a `u64`
- **Seed detail (27 September 2026):** since 25 September 2026 (#137), for a record, a number type and a pointer type.

### `:` reads a name's binding, `.` reads a thing's own fields
- **History:** until 29 September 2026 code walking the graph asked a named operand's binding "`:scope`, `:gate`, `:type` directly". `:type` left the binding's reads with #199: a value's type is read `.type`, from the payload.
- **History:** 7 September 2026: the binding held `dyad`, `scope`, `start`, `end`, `gate`; `a:dyad` was its cell. 23 September 2026: `:dyad` retired (see `a:type` above). The `b:start.rhs.type` example corrected 8 September 2026.
- **Seed detail (27 September 2026):** since 8 September 2026 (#70): every trie entry is a dyad of type `binding`, a use stores it, `:` reads its fields. A constructed node answers `:scope` with the scope open at the read and `:start`/`:end`/`:gate` null, the enclosing item being the segment still under construction.

### `x:name` is the name's spelling, held by the binding
- **Seed detail (27 September 2026):** since 14 September 2026 (#120): the binding's sixth slot, a place of type `string` holding the name node. `x:name` reads it as the container it is; the display shows its text. A constructor reaches it for the binding a cell holds as `tape[k]:name` (always the identity's name, never the appearance's text, which is `tape.spelling[k]`).

### A binding's fields are read at elaboration, at the line of the read; running code never reads a binding
- **History:** until 29 September 2026 the list of binding reads began "`a:gate`, `a:scope`, `a:type`, …". `a:type` left it with Thobias's ruling that a value's type is read `a.type`, from the payload, not the binding (›`a.type` reads the type of the value a name stands for (spelled `a:type` from 23 to 29 September 2026)‹, #199).

### A `type ?` place is a box, written any number of times
- **History:** before 12 September 2026 the first `a = i32` rebound the name to the type, making it define-once; a second assignment said a type is not an assignable place, because the name by then *was* the type.

### The pass runs only as far as it must, in order, and never twice
- **History:** 13 to 28 September 2026: what ran stayed in the graph as `{type: ran, value: [expr, value]}`, rewritten in place over the item (the seed's `ran::rewrite`, R3, #88). Superseded 28 September 2026 by the frame's cursor and the result on the stack, because a run must not rewrite the shared graph and a scope's "how far" is per call. Seed: the `ran` rewrite is to be deleted (#167).
- **History:** until the evening of 28 September 2026 the deferred-scope sentence ended "nothing in it runs in the pass", with no exception; amended when `immediate x` was ruled to run inside a deferred body too.

### `immediate x` runs the expression to its right as soon as it is parsed, and stands as its value (spelled `run x` earlier on 28 September 2026)
- **History:** spelled `run x` from the morning of 28 September 2026 to the evening, as a second word beside the slot word; respelled `immediate` because one spelling for two words made `run := 5` at the root an error and would have needed every reader to sleep before `:=`. Thobias asked for "some other name for run x", a word "for saying something should be done right now in the moment"; `immediate` was chosen over `now`, `settle` and `once`.
- **History:** 28 to 29 September 2026 the text read "Inside a deferred body too: `f := fn () -> i32 ( immediate bump() )` runs `bump` once, as the body parses, and the body holds the value; an operand that reads a name of that body, a parameter say, is the ordinary read-before-written error." Superseded 29 September 2026 (#197) by "when that body is parsed", per build, because a type's `run` body and a `type (…)` in a body are lexed where written and built later, some more than once, where "once" and "a parameter … is the read-before-written error" gave different answers; Thobias: "immediate executes right when its parsed".
- **History:** on 29 September 2026, on the #197 branch before it reached dev, the #197 Ruled line read "A type's `run` body is parsed when a node of the type is built, once per set of field types, even when those types are known at the definition: `sq2 := …` prints nothing until the first `sq2` node is built (#197's option C, parsing such a body at the definition, not taken)". Narrowed the same evening by the `fn` and `run` Ruled line, for the reason given under ›A generic function body is lexed once and built once per set of field types‹.

### `dyad ?` is the general box, `type ?` the narrow case
- **History:** written `a:dyad.type == type` until the 23 September 2026 respelling.
- **History:** written `a:type == type` from 23 to 29 September 2026, when a value's type became `a.type` (#199).
- **History:** 12 to 28 September 2026: the box's mark sat on the value word (the seed's bits 63 frame, 62 global, 47 arena). Superseded 28 September 2026 by ›A scope lays out its declarations; a use reaches the offset through its binding‹, because the frame is per call, the offset per name, and a node's value word should be the value.

### A `dyad ?` place is transparent: a placeholder for a new node of any type
- **History:** the rule's first Open line (29 September 2026) held seven points. The same evening Thobias ruled five: `a:type` is the held node's type; `-> i64 ( a )` is checked where the call is parsed; a second write replaces the node; a literal keeps its own type; and the 28 September rejection at ›A `type ?` place is a box, written any number of times‹ holds for the name `id` only. Three were asked again, because his answers left them unclear or he lacked the context: what an argument that must run leaves in `a`, whether a held `rational_number` lands in `-> i64`, and when a build made after `compile f` is compiled.
- **History:** the second round's Open line (29 September 2026) held those three points; they were ruled the same night, each recorded at the rule its reason belongs to. The rule's `a:type` examples were respelled `a.type` with the ruling of the same night.

### A scope lays out its declarations; a use reaches the offset through its binding
- **History:** before 28 September 2026 the seed kept a name's storage as a place node the binding pointed at, its value word marked (bit 63 a frame offset with the lexical depth beside it for the capture guard, bit 62 an absolute address, bit 47 an arena offset); a nameless result (an instance, a call's record result, a rational step, the receiver of a `share` call) got a marked place of its own at parse; between #167 and #168 the program frame's cursors stood on the runtime. Superseded 28 September 2026 by the rule and its five points, because a frame is per call, an offset per name, a result nobody named is nobody's, and a node's value word is the value.
- **Seed detail (29 September 2026, #168):** a `share` function called through a plain record took a node minted per call, `[T][marked bytes]`, as its value; now the record's address, the field reads carrying the owner type. A function's parameters were placed by writing the frame offset into each parameter node's value word; now each parameter's binding is laid out and the argument block's walk derives the same offsets. A held `type (…)` remembered the depth of the open functions; now the `fn` nodes themselves. The per-function `alloc_local` counter is gone with the marks.

### A tape cell checked to be a `type` passes as a `type`
- **History:** the text said "Only a check against `type` narrows: what a read narrowed to a number type would yield is not ruled"; superseded the same day by the next rule.

### A tape read checked against a number type reads as that number
- **History:** before 25 September array.logos wrote `if not tape[1].dyads[0]:type ⊆ this.size:type error «…»` and passed the index to `this.at` as a `u64`; changed by *An index may be of any integer type*.
- **History:** until 29 September 2026: "A literal line has no committed type ("a concrete type beside a literal molds the literal to it"), so `⊆ T` answers for a literal by whether it molds into `T`". Reworded by Thobias's ruling that a plain number is a `rational_number` which the number type converts (#199: "rational_number is already a type"); what `⊆ T` answers for a literal is unchanged.

### A block settles its boxes as the top level does
- **History:** 12 September 2026: the stores filling such a box were replayed at parse, in parse order (the right side a node address fixed at parse, so the run stored the same bits again). Replay superseded 13 September 2026 by *The pass runs only as far as it must*: nothing is replayed.

### A `type (…)` inside a body is built once per set of the values it reads (built each time it runs, until 29 September 2026)
- **History:** 25 to 29 September 2026 the heading was "A `type (…)` inside a body is built each time it runs" and the text read "each time its node runs the parser builds it from those cells with that call's values live. So `mk := fn (t := type ?) -> type ( type ( fields = ( shared e := t ) ) )` gives a new type per call whose `e` is that call's `t`." Superseded 29 September 2026 (#197) by one build per set of the values it reads, because it makes a written type a template over what it reads, and types are created too seldom for the speed of a build at every run to matter (Thobias: "i would lean towards b since that allows for more general templates for types").
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): a held `type (…)` builds a fresh type on every run, so one type per element type is array.logos's `array_mints`, not the seed's.

### The chooser keeps its mints in a memo, and the driver constructs the mint
- **History:** 18 September 2026: memo `array_mints` a plain top-level variable beside the chooser, read by its `parse` as an outer name (a top-level name read in a body is the shared place itself, *Memory and concurrency*; a call is a use of every outer name, #125); the chooser ran the placed cell's `parse` itself. 19 September 2026: the driver runs it by the flag. 18 September 2026: reinstating `shared` for a fields-block member proposed and withdrawn within the minute (the memo is not instance data); the withdrawal superseded 19 September 2026 (`shared` reinstated as the mark of placement, *Two muts, and the storage partition*). array_mints.logos folded into array.logos 19 September 2026. 25 September 2026: memo moved into `get_mint`.

### A generic function body is lexed once and built once per set of field types
- **History:** August 2026: the body was "parsed once with unresolved op slots that a call's concrete types fill, walkable meanwhile through `run`'s null-`bcode` path". The text's example was spelled `shared run = (…)` (before *Fields block collapse*, 25 September 2026, which gives `^.run`).
- **History:** on 29 September 2026, on the #197 branch before it reached dev, the text read "A type's `run` body is held and built this way even when its field types are known at the definition: `sq2 := type ( a := i32 ?, share run = (…) )` builds its run body when the first `sq2` node is built", from Thobias's first #197 answer. Narrowed the same evening by his second: a `fn` body and a `run` body are built at the definition when the build has all it needs there, because under the first a `fn` and the type it spells built one body at two moments ("when it actually can be built at definition it should be built").
- **Seed detail (27 September 2026):** #126; since #133 slice 8: the body held as lexed cells and constructed per set; a bare literal in an untyped field gives the `rational_number` set, whose body runs over rational places and operators, interpreted only; a node whose value fields are all literals folds at construction to its literal.

### The command line is Logos source; the binary stays out of the way
- **History:** `logos file.logos` superseded August 2026: ambiguous against the since-retired `.logos` metaproperty (`file.logos` read as "the logos of `file`").
- **Seed detail (27 September 2026):** converged August 2026 (#58): argv-as-source with file-path mode removed, `import` with pub-only exposure, once-per-run DAG loading, the fresh-view section boundary (command line and REPL each run as their own section), file-relative path resolution.

### `import` is the one identity that loads a file
- **History:** l.221 had "v0.1.0 has no gates and an importer sees every top-level name" (4 September 2026); superseded.

### `print «…»` is the output word
- **Seed detail (27 September 2026):** since 24 September 2026 (#140): each run writes the quote and a newline and yields unit, a statement the echo rule keeps silent.

### A type body describes one level
- ›A type body describes one level‹ merged into ›A type body describes one level‹ on 27 September 2026
- ›A value never has its own parse; the only parse is written in a type body‹ merged into ›A type body describes one level‹ on 27 September 2026
- ›There is no `this`: a type's bodies name fields directly; a parse builds its node in `tape[0]`‹ merged into ›A type body describes one level‹ on 27 September 2026
- ›In a parse, a field is always written through its cell‹ merged into ›A type body describes one level‹ on 27 September 2026
- **Seed detail (27 September 2026):** one level since 26 September 2026 (#150): a type body is read as one list of fields, share members, slot fills and prose; a type stores one parse, rank and associativity; a value that appears wakes its type's parse; a node a Logos parse placed for its own type is not woken again; `^.run` and `point.dims` read through the type; the fields-block errors are gone; prose before a field or member is read past, before a slot fill it is kept beside it (#150 made a bare slot fill legal and `shared parse = (…)` the checked error, a rule superseded by the `share` ruling, ›Two muts, and the storage partition‹); no `this` and the cell write: not yet (#153), per DESIGN.

### `array` is a chooser
- ›A generic type is written by juxtaposition: `array i32`‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›The chooser keeps its mints in a memo, and the driver constructs the mint‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›The array is two levels, not three‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›Mints stay: the element type is stored once, in the mint‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›The memo and the minting are `array`'s own members‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›`array array i32 [ … ]` needs no brackets around the element type‹ merged into ›`array` is a chooser‹ on 27 September 2026
- ›`array ?` is an empty node of `array`‹ merged into ›`array` is a chooser‹ on 27 September 2026
- **Seed detail (27 September 2026):** the rank numbers are already the seed's (application and an unfilled `share parse_rank` 91, `fn` 92); two blockers were recorded open and closed 25 September 2026: a parse written in Logos running at discovery could not read past what is lexed (the #61 stand-in, closed by the lazy-read ruling), and the chooser reading `i32` would wake `i32`'s own read of the bracket (closed by the unbuilt-cell ruling: the chooser gets `i32` unbuilt and `(1, 2, 3)` as a bracket; "just below it" means a bracket is built as it is lexed and a juxtaposition is not); as written on 25 September, array.logos still stopped at `tape[0] = get_mint(t)`, a cell read from the tape not being taken as a `type` argument; `array array i32` and `array ?` done in #151.

### An array expression is a node of `array`, and its run makes the value
- ›An array expression is a node of `array`, and its run makes the value‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›Each evaluation of an array expression makes a new array‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›A list's elements are filled when the code runs‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›Elements are built by assignment‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›The element type decides how a line becomes an element‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›The array's list is written in square brackets‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- ›A value holds no spare capacity‹ merged into ›An array expression is a node of `array`, and its run makes the value‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026 (#137) `array i32 [x, y, 3]` fills at run, a function that makes an array compiles (the bracket's number lines run in the compiled frame; the new bracket and the per-run copy, nodes of the store, are made by a call from machine code back into the seed; the builder runs interpreted as any uncompiled callee does), and a return type spelled `-> array i32 ( … )` stops before the body's bracket (the mint's `tape[1]` reads `void`), so `mk := fn () -> array i32 ( array i32 [1, 2] )` returns the array; the square-bracket list is in identities/array.logos since 25 September 2026, no Rust change; since 26 September 2026 (#151) `array T [ … ]` is a node of `array` whose run makes a new value each time, `array`'s parse leaves the mint when no bracket follows, `v := T ?` of a type whose values are nodes is a new empty node, an untyped field holding a bracket has its lines evaluated at each run where the node stands, and a node a Logos parse placed, of any type, is not woken again by its own type, while the value its run yields wakes the value's type, so `array i32 [4, 5][1]` reads 5; stand-in for #152: a square bracket taken by `=` into a place of a mint (`x = [4, 5]` and each line of a nested list) is built in Rust as an `array` node over it, and #152 recognises a mint as a type minted while `array`'s parse runs and reads its element type from its `element_type` member.

### A value of a type built by a Logos `parse` travels as a pointer
- ›A value of a type built by a Logos `parse` travels as a pointer‹ merged into ›A value of a type built by a Logos `parse` travels as a pointer‹ on 27 September 2026
- ›A field of such a type holds the node's address‹ merged into ›A value of a type built by a Logos `parse` travels as a pointer‹ on 27 September 2026
- ›`alloc n of T`, for such a `T`, gives n cells of its address‹ merged into ›A value of a type built by a Logos `parse` travels as a pointer‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026, no array-specific code: a function holding the field read stays interpreted, as every field read does (only the per-run copy lowers), and a compiled callee handed `b.items` runs compiled; one test in `read.rs` for which cells load whole (`cell_numtype`); pointer stepping since 25 September 2026 (#137), pointer on the left, integer on the right, k scaled to bytes as an `i64` product in the graph, both tiers, with `k + p`, `p - q` and every other pointer operator refused.

### A value owns what its elements hold and frees it
- **History:** ›A value owns what its elements hold and drops it‹ until 28 September 2026.
- ›A value owns what its elements hold, and drops it‹ merged into ›A value owns what its elements hold and drops it‹ on 27 September 2026
- ›A list's lines move into the value built from it‹ merged into ›A value owns what its elements hold and drops it‹ on 27 September 2026
- **Seed detail (27 September 2026):** the refusal stands where a type's parse hands a bracket to the call it places, for every type written in Logos; a callee that only reads its list is refused alike.

### `a[k]` is an application, exactly as `a(k)`
- ›Element access is `[…]`, application is `(…)`‹ merged into ›`a[k]` is an application, exactly as `a(k)`‹ on 27 September 2026
- ›`a[k]` is an application, exactly as `a(k)`‹ merged into ›`a[k]` is an application, exactly as `a(k)`‹ on 27 September 2026
- ›An index may be of any integer type‹ merged into ›`a[k]` is an application, exactly as `a(k)`‹ on 27 September 2026
- ›A call that ends in a dereference is a place‹ merged into ›`a[k]` is an application, exactly as `a(k)`‹ on 27 September 2026
- ›`a` need not be `mut` to write its elements‹ merged into ›`a[k]` is an application, exactly as `a(k)`‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026 any integer index in array.logos and in the placed call's read of a narrowed line; `=` over a call ending in a dereference builds a copy of `f` whose body ends in `p` and returns `@T`, and stores through that copy's call, no array-specific code; a fault a compiled caller parks is the first one, so the null guard over the zero an interpreted callee's error left does not replace that error.

### A bracket goes to the call whole
- ›`[…]` builds itself; the collection consumes it‹ merged into ›A bracket goes to the call whole‹ on 27 September 2026
- ›`[` is `(` in square brackets, and lands a `square_brackets` cell‹ merged into ›A bracket goes to the call whole‹ on 27 September 2026
- ›A bracket goes to the call whole‹ merged into ›A bracket goes to the call whole‹ on 27 September 2026
- ›At top level, a `[…]` line of a list is not run early‹ merged into ›A bracket goes to the call whole‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026 (#137) `[` lands a `square_brackets` cell whose `dyads` hold its lines, the seed's `[` still folding `t[k]` in its own constructor, a tape value having no constructor of its own yet; the top level's pending run skips a `[…]` line since 25 September 2026.

### A tape read checked against a number type reads as that number
- ›A placed call keeps its tape lines as operands‹ merged into ›A tape read checked against a number type reads as that number‹ on 27 September 2026
- ›A tape cell checked to be a `type` passes as a `type`‹ merged into ›A tape read checked against a number type reads as that number‹ on 27 September 2026
- ›A tape read checked against a number type reads as that number‹ merged into ›A tape read checked against a number type reads as that number‹ on 27 September 2026
- ›Inclusion between types is `⊆`‹ merged into ›A tape read checked against a number type reads as that number‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): both narrowings and the placed call's kept lines; `⊆` between two types, folded at parse when both are known, run interpreted when one is read at run (a tape cell's `:type`), a type includes itself, two integer types compare by value range (`u8 ⊆ u16` and `u8 ⊆ i16` true, `i8 ⊆ u64` false), and any other pair (float and integer, two floats, `bool`, a record, `type`, `dyad`) is a checked error, what those types hold not being settled here.

### A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type
- ›A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type‹ merged into ›A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type‹ on 27 September 2026
- ›A name standing with nothing to its right wakes and stands‹ merged into ›A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type‹ on 27 September 2026
- ›A node a parse built runs, and is never parsed again‹ merged into ›A name of a type with an instances' `parse` wakes it, as the instance does, and so does any expression of the type‹ on 27 September 2026
- **Seed detail (27 September 2026):** since 25 September 2026 (#137): names, parameters, call results and brackets wake, interpreted and compiled; `a[1]`, `b := a, b[1]`, and `f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)` work, the last also after `f.compile()`; a field read wakes too (later on 25 September; before, `this.items[1]` over a field was refused).

### A `tape[k]` read past the lexed frontier lexes on demand for every constructor
- ›A `tape[k]` read past the lexed frontier lexes on demand, for every constructor‹ merged into ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹ on 27 September 2026
- ›A cell a constructor reads from the tape arrives unbuilt‹ merged into ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹ on 27 September 2026
- ›A reader's read builds an equal right-associative cell first‹ merged into ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹ on 27 September 2026
- ›A read where the tape reaches no cell gives a `void` node, already constructed‹ merged into ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹ on 27 September 2026
- **Seed detail (27 September 2026):** 25 September 2026 (#137) for the lazy read, the unbuilt cell and the right-associative read, in the driver's lazy read with no array-specific code; 25 September 2026 (#147) for `void`: a tape read to its end point is sealed, so a later read past it gives `void`.

### The seed's tape shape (#60, #121) and its remaining stand-ins
- ›Seed tape shape (#60, #121)‹ merged into ›The seed's tape shape (#60, #121) and its remaining stand-ins‹ on 27 September 2026
- ›Seed stand-ins that remain‹ merged into ›The seed's tape shape (#60, #121) and its remaining stand-ins‹ on 27 September 2026
- **History:** until 29 September 2026: "a fresh dyad for an unknown spelling" among what a cell may be. The fresh dyad went with the one-word node (#166, ›An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value‹); the sentence followed on 29 September 2026 (review round 2 of #166).

### Slot words are known only inside a type body
- **History:** 19 to 28 September 2026: `run := 5` at the root was an ordinary declaration. Since 28 September 2026 `run` is also a word of the language start (›`run x` runs the expression to its right as soon as it is parsed, and stands as its value‹), so it is the no-shadowing error there; the slot still wins inside a type body.
- **History:** later on 28 September 2026 that word was respelled `immediate` (›`immediate x` runs the expression to its right as soon as it is parsed, and stands as its value‹), so `run := 5` at the root is an ordinary declaration again. Reason: one spelling for two words made `run := 5` an error and would have needed every reader to sleep before `:=`.
- **History:** 28 to 29 September 2026 the Ruled line said without exception "no root word shares a slot word's spelling", though `free` (`drop` until that day) was already one word with a root word by ›`free` is one word‹. Since 29 September 2026 `print` is a slot word as well as the output word, one word as `free` is (#174), so the sentence holds for every slot word but those two. Reason: `print` was already a root word, so the slot makes no root declaration an error, and one word whose parse looks to its right needs no reader to sleep; Thobias kept the name because "print could also mean print text in a string and return the string".

### Importing is dropping the text there, wrapped in its own scope
- **History:** until 28 September 2026 the seed gave an `import` line the imported file's tail value (the command line and the REPL echoed it, held in a `ran` node). Ruled valueless 28 September 2026: a file is its own graph, the importer gets its `pub` names, and no result is kept outside the stack.

### A dyad is a type and a value: one block, the type word first, and its identity is its address
- **History:** "`x:type` reads the first word" until 29 September 2026, when a value's type became `x.type`, read from this block like any field (#199).
- **History:** until 29 September 2026 the heading read "two pointers, and its identity is its address" and the rule opened "Sixteen bytes. Any allocator can hold it (no arena needed; the address is the id)": a node was a 16-byte cell of two pointers, type and value, the value's bytes behind the second. Superseded by the one-word node, Thobias, 29 September 2026 (#166): a node is one thing, and the seed loses the typeless node, the placeholder copy and the second space; the measured gain was only about 1.2×, so bytes were not the reason.

### Operands sit inline after the type word; a growing list stays behind a pointer
- **History:** until 29 September 2026 the heading read ›Operands are reached through `value`, never stored inline‹: "A binary `+` is one 16-byte cell pointing at its operands", `value` pointing at a separately allocated operand record, and "A list is never inline: a sequence's expression array sits behind one pointer, an `array` value the scope dyad's value points at"; the reason was the fixed cell. Superseded by the one-word node (#166): fixed-arity operands inline, growing lists still behind a pointer.

### An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value
- **History:** until 29 September 2026 the heading read ›An unknown spelling lexes to a fresh dyad with both slots `undefined`‹ (ruled 2 September 2026): "The cell holds a pointer to the fresh dyad; the spelling is kept tape-side. `:=` fills that dyad and enters the spelling into the trie at declaration (`key := ?` of *Declarations are immutable by default* binds exactly such a dyad)." It was the one rule that made a node before its type. Superseded by the one-word node (#166), which cannot fill a node born without its size; the binding is the early home instead. Seed at the ruling: the fresh null node in src/parse/lex.rs and the placeholder copy in declare_here (src/parse/declare.rs, the first half of #179) were what this rule licensed.

## Identity recognition

### How an operation is defined for its operand types is OPEN
- **Seed detail (27 September 2026):** hard-codes it for core types. `=` is handed to the `parse` of the type of the place on its left (the l.189 sentence), which stands as seed practice but is in question. A square-bracket literal goes into an array's place in Rust, like number literals. Seed and spec issue #152.

### A scope node stores only its enclosing scope, spelled `back`
- **History:** link spelled `.scope` (`here.scope.scope`) until 24 September 2026.
- **Seed detail (27 September 2026):** since #123 every scope is minted `[exprs, op, parent]`; `ScopeStack.open` is the cache. `.scope` on any `@dyad` value reads the link when it runs (`.back` since 24 September 2026), null at the arche, checked error past it. Addresses compare with `==`.

### `here` is where a line is written; `caller` is where it was called from
- **History:** `this` joined them 17 September 2026 (a name the slot's type declared, from 18 September); `this` is gone since 26 September 2026 (l.207).
- **Seed detail (27 September 2026):** `here`, `caller` are identities. `here.scope` folds to the scope open at the appearance, as `x:scope` does. `caller.scope` answers the pass's position while a constructor runs, checked error elsewhere: stand-in for the per-call read. Not carried: bare `caller` as a value; the section-boundary stop (waits for gates); a one-item group, which the seed elides, so `here` inside one names a scope no path reaches.

### The meta-access surface is `:` and `.`
- **History:** 7 September 2026: `:`, `.`, and a `back` step walking up the written path one `.`/`:` at a time; retired 24 September 2026.

### A type with a canonical form ships a normalizer; equality there is identification
- **History:** conjecture respelled 7 September 2026; `where` read `x:dyad.type == T` until 23 September 2026.

## Identities, names, and metadata

### The name index maps a spelling to a list of bindings
- **History:** `id_context` → `record` 7 September 2026 (seed `IdContext` → `Record` 8 September, #67) → `binding` 24 September. `record` remains only for: an operand record, a record type, a type's own metadata head, the archival sense. Spelling and `lex_rank` on it since 14 September; a use points at it since 8 September 2026.
- **Seed detail (27 September 2026):** `record`/`Record` and the README followed as `binding`/`Binding` 24 September 2026 (#138).
- **Quotes:** Why `binding`: "i think binding is best".

### The binding carries the name's gates; a partial move adds a sub-range
- **History:** gates ruled 5 September 2026 as a separate identity binding; merged into the one binding per name 7 September 2026.

### Metadata has three homes, by frequency and origin
- **History:** home (1) was four construction-phase tag bits per node, superseded 7 September 2026 (`undefined` is a null slot, writability a gate). A derived id → metadata index superseded 8 September 2026: no address-keyed index; facts come from name or path. Gates: separate identity binding 5 September, one binding 7 September.
- **History:** "node stays 16 bytes" until 29 September 2026 (the one-word node, #166).

### Lookup rides the scoped walk; the index only speeds things up
- **History:** derived id → metadata index for that case, superseded 8 September 2026.

### The store is keyed by address
- **History:** until 28 September 2026 the seed read "the address is an index" as a raw `*mut Dyad`, the cell two raw pointers; the 32-bit index handle behind store accessors superseded that reading (#165).
- **History:** 28 September 2026, later the same day: the 32-bit index handle (address = base + index × 16, cell head a 32-bit type index) was withdrawn as a misconception, Thobias had asked whether the agent proposed it, it had not, and he never wanted it; the handle is the 64-bit address, and the cell head's 32-bit type index went with it the same day, no reason for it being his either: the cell is two full pointers again. Seed: #165 slice B's flip was cancelled before it was built; its accessor pass stays, being layout-neutral.
- **History:** until 29 September 2026: "Nodes are fixed two-pointer (16-byte) cells, so the address is an index: no key, no hash", and an Open line of 28 September holding the one-word node back "while a fresh cell is stamped (`tape[0]:type = T`) after it exists, since a block cannot be allocated before its type is known". The blocker was stale (›How a Logos constructor builds its node‹ replaces the cell with a fresh node of the assigned type, and the seed births it typed) and the layout was adopted the next day (#166). Superseded record: the seed's `Store::contains` grid check, five sites, guarded what the types refuse.
- **History:** until 29 September 2026 the Why of the #166 ruling ended with the example "(`mut d := dyad ?, d = 5` is a parse error)". Superseded the same day by ›A `dyad ?` place is transparent: a placeholder for a new node of any type‹ (Thobias, #199): a write into an empty `dyad ?` writes the written value's type, then the value, so the example no longer shows a refusal: `d = 5` writes a `rational_number` node (Thobias, #199: "a plain number is type rational_number").

### A uniform model is not uniform storage
- **History:** "(16 bytes; handle = address)" until 29 September 2026 (the one-word node, #166).

### A dead name takes nothing; only `:=` may follow
- **History:** until 29 September 2026: the Rejected line's reason read "the right `x` is the new placeholder". The placeholder went with the one-word node (#166, ›An unknown spelling stays text on the tape; `:=` makes the binding, and the node comes with the value‹): the right `x` is the new binding with no node yet; the wording followed on 29 September 2026 (review round 2 of #166).

## Standard library and ecosystem strategy

Nothing moved from this section.

## Tooling philosophy

Nothing moved from this section.

## The IDE as platform, and the reflective web

Nothing moved from this section.

## Englogos: a Logos-native human language

### First surface sketch: a split copula, and tense as a stack of reference points
- **History:** a push-simultaneous `o` deleted as redundant (moved depth, never time). Interim `paoil` as "eternal" superseded: its set reading cannot survive sequence semantics; eternity is quantification over times, not a marker.

## Chain programmability: constrained targets as backends

Nothing moved from this section.

## Durable state: checkpoints, caches, and the store

### A durable store, in files, managed by Logos code alone
- **Seed detail (27 September 2026):** owes only file syscalls, already in the `native` floor. Formats, manifests, compaction, policy are stdlib or third-party Logos; the manifest is graph values.

## Feasibility and effort

The effort table:
- **History:** dependent types were "not needed for an initial release" until 5 September 2026.

### How a Logos constructor builds its node (step 3's first question)
- **History:** 9 September: `tape[0]:dyad.type = ^` then `tape[0]:dyad.value.operands.append(tape[-1] and tape[1])` (the and-group distributing so both append, *The proof layer* 7 September; that and-group ruling stands on its own); the operand record was the null-terminated run call arguments travel in. 16 September: named fields. 17/18 September: `this.lhs = tape[-1]`, `this` placed by `tape[0] = this`. `this` gone 26 September (l.207); `:dyad.type` → `:type` 23 September.

### The LSP server comes after v0.1.0
- **History:** before: "A thin LSP server gates v0.1.0's public release, not the demo."

### Staged out of v0.1.0: error values, and writing a name's dyad view
- **History:** 2 September: view "fully readable and writable". Spelled `:dyad` (`tape[k]:dyad.type = T`, `x:dyad.type = f64`) until 23 September 2026.

### Gates before v0.1.0: `pub` stays and `mut` lands
- **History:** 4 September 2026: v0.1.0 has no gates (no `pub`, no `mut`; all readable and writable; `=` writes any declared name; an importer sees every top-level name), because a gateless seed is smaller; `pub`-only exposure, `mut` as type modifier and the fail-closed default were to land together after v0.1.0. The seed's `pub` gate slot and pub-only import tests were a divergence pending removal until 20 September.

### The v0.1.0 demo: a user-defined power operator `^`
- **History:** computation first in a `code` slot as an ordinary function (now `run`). Parameter first `x : i32` (superseded colon form), respelled the same day; the `:`-as-sugar door of *Declarations are immutable by default* stays open but unused. "`fn`-typed parameters carrying a tape" superseded 17 September 2026 by `tape` as a word in `parse`.

### `^` takes floats and any real exponent
- **Quotes:** Ruled: "Any real exponent"; Ruled: "written in Logos".

## Closing

Nothing moved from this section.
