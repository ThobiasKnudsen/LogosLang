# Logos: A Unified Programming System (DESIGN2)

**What this is.** A rewrite of DESIGN.md (dev 2f7adc6, 27 September 2026) with the same content in a findable shape: one rule per heading, the live rule first in plain present tense, then its reason, its ruling, what was rejected, what is open, what the seed does, and its history, each on its own line. DESIGN.md stays the ruling document until Thobias says otherwise. Where the two disagree, DESIGN.md wins and this file is wrong. Nothing here is a new ruling.

**How to read a rule.**
- The heading is the claim; the text under it is the rule as it stands today.
- **Why:** the reason recorded with it.
- **Ruled:** the date and, where the text says, who. "Claude's choice, open to Thobias" is kept visible.
- **Rejected:** alternatives turned down; "to stay rejected" where DESIGN.md says so.
- **Open:** what is not decided.
- **Seed:** what the Rust seed does today, its stand-ins and issue numbers.
- **History:** one line per superseded rule: the old rule, when it changed, why.
- **Source:** the DESIGN.md line(s) the rule comes from. DESIGN.md's `##` sections stand here in the same order, so a section is found by its old name.
- **CONFLICT:** two passages of DESIGN.md that both read as live and disagree; both are quoted. These are questions for Thobias, not rulings.

**Spellings.** Live rules use the spellings ruled by 26 September 2026: `a:type` (not `a:dyad.type`), `share` (not `shared`), no `this`, no `fields = (…)` block, `run`/`parse`/`drop` as the slot names, `binding` (not `record`). Old spellings appear only in History lines and dated quotes. Some examples were rewritten into today's spelling where DESIGN.md never wrote them that way; those rewrites are the editors' reading and not rulings:
- `a:type is number`, `a:type == i32`, `(a and b and c):type == i32` in the conjecture and `fn` examples (l.69 wrote `:dyad.type`); `share array_mints` (l.34 wrote `shared`).
- `f.run` for `f.code` in the `call` access kind (l.107, l.108); bare `free ptr`, `drop items`, `items = …` and `tape[0].items = …` for `this.items` and `free this.ptr` (l.104); `x:type` for `x:dyad` (l.107, l.108).
- `share` for `shared` in the l.121 and l.131 examples; `p:type` for `p:dyad.type` (l.123); `mut &a` and "write-through permission" for `&mut` (l.123); `tape[0].f = …` for `this.lhs = tape[-1]` (l.129); `point.dims` for `point.fields.dims` (l.133).
- `tape[0]:type = ^` then `tape[0].lhs = tape[-1]`, and `tape[0].output_type = tape[-1]:type` (l.189 wrote `tape[0]:dyad.value.lhs = …`, `this.lhs = tape[-1]`, `tape[0]:dyad.value.output = tape[-1]:dyad.type`); `share parse_rank = mul.parse_rank - 1`, `share run = (…)`, `share parse = (…)` (l.176 and l.189 wrote them bare or with `shared`).
- `(ptr + i)@ = elements.dyads[i]` for `(this.ptr + i)@` (l.203); `if.parse` for `if.constructor` (l.209); `share parse_rank = *.parse_rank + 1`, `share associativity = right`, `share element_type := t` (l.201 wrote them bare or with `shared`).
- `array T [ … ]` for `array T (…)` throughout l.211, where the text itself says to read them so; bare `size`, `ptr`, `element_type`, `at` and `tape[0].at(k)` for `this.size`, `this.ptr`, `this.element_type`, `this.at(…)` (l.211); `share at` for `shared at` in a History line.
- `a:type` for `a:dyad.type` (l.213, l.217); `run = (…)` for `shared run = (…)` (l.219); `x:type == T` in the normalizer conjecture (l.249).
- `tape[0]:type = ^`, `tape[0].lhs = tape[-1]`, `tape[k]:type = T`, `x:type = f64` in the preview-milestone rules (l.375 wrote `this.lhs = tape[-1]`, `tape[0] = this`, `tape[0]:dyad.type = ^`, `tape[k]:dyad.type = T`, `x:dyad.type = f64`).
Kept in the old spelling because the current one is not written anywhere: `mk := fn (t := type ?) -> type ( type ( fields = ( shared e := t ) ) )` and `shared array_mints := hashmap type -> type` (l.217); the array.logos snippets with `this.size`, `this.at`, `this.element_type` in the narrowing rules (l.217); `a:dyad` "in the frame is the matched node" (l.69); `f:dyad.value.body[0].rhs` (l.241); `self` in the refinement example (l.58).

**Conflicts found by the rewrite**, each marked CONFLICT at its rule, with both passages quoted there:
1. l.71 against l.69: the truth rule as the primitive, against two recognizers, two conjectures (›Truth rules and value rules‹).
2. l.99 against l.75: the ecumenical system composing with the universe kernel, against the universe tower being no longer live (›Branches and universes‹).
3. l.108 (15 September) against l.107 (20 September): "the defaults admit" a gate write, against the default write entry being gone (an Open under ›A gate is a node of the body‹).
4. l.127 against l.135: a name may go `mut`, `immut`, `mut` again, against "the reverse flip" rejected to stay rejected (›`immut x` removes `mut`; `lock` seals the gate set‹).
5. l.127 against l.107 and l.129: the default gate "write allowed while the slot is null", against no default write entry (same rule).
6. l.119 against l.107 and l.123: `key := T ?` then `key = value`, and `v.f = …`, shown without `mut`, against a name without `mut` being written nowhere (›CONFLICT: `=` refused on a non-`mut` name…‹ at the end of ›Mutability and construction‹).
7. l.131 against l.207: `share` refused on a type body's own line, against `share parse = (…)` being required there (›`share` means one place…‹).
8. l.145 against l.189: constructor signatures `-> void`, against a parse body having no parameter list (›Target: a constructor returns `void!`…‹).
9. l.178 against itself: the `,` between every two expressions, against "explicit only where adjacency would otherwise read as consumption" (›`,` outranks a type's optional operand…‹).
10. l.180 against l.189: only a `fn` is ever run, against a type with `run` running (›Hosted text does not owe a single reading‹).
11. l.191 against l.205: `[ ]` reserved for indexing, against the array's list written in `[ ]` (›Brackets divide by role‹).
12. l.189 against itself, recorded open in DESIGN.md: whether `f.compile()` reaches the code of a type its body uses (›`fn` is not a primitive…‹).
13. l.201 against l.201 and l.207: a field checked against its siblings only, against no shadowing, once fields and body lines are one list (›Fields are read with a dot outside their scope‹).
14. l.211 against l.203: `a[k] = v` writes into the array, approved and in the seed, against the same question recorded open the next day (›A call that ends in a dereference is a place‹).
15. l.221 against l.375: v0.1.0 has no gates and an importer sees every top-level name, against `pub` staying and `mut` landing before v0.1.0; l.213's "view ungated in v0.1.0" may share the root (›`import` is the one identity that loads a file‹).
16. l.241 against l.213 and l.189: `a:type` stays the checked error, against `a:type` being the read (›A constructed node answers `:` from the path…‹).
Noted without a CONFLICT mark: `share` now names both the co-owning reference word (`share @T`) and the placement mark; `immut` both removes an entry and is a veto; `?` is a run-time value (l.119) and also "inert, making any build that reaches it incomplete" (l.135); l.34 calls `hashmap` pending where its seed note says it works; l.207 says "Seed: not yet (#153)" where dev b2b854c has it; l.375 places `exp` and `ln` in power.logos where d9a78cd moved them to their own files.

**Size.** DESIGN.md is 455 KB in 383 lines; this file is about 400 KB in 384 rules. The rules are the same rules. The saving is in finding them, not in their number; a real cut needs decisions on what may leave (dated quotes, per-rule seed status).

### How to read this document
Top-down: vision and architecture first, then detail. Some later sections (identity recognition, representation internals) describe a target not yet tested against a real corpus; they are flagged where they appear. A live rule carries *ruled* or *settled* plus a date; a dead one is marked *superseded*. Search for those words to list either.
- **Ruled:** 2 September 2026 (reading convention). Who is not stated.
- **Source:** DESIGN.md l.3

### Versions are named vX.Y.Z
**v0.1.0** is the preview, the first public milestone. **v1.0.0** is the release, the stability promise. The *seed* is the Rust program that runs at every version until self-hosting replaces it; it is never a version.
- **Why:** the old words *v1* and *Core* each had two readings, and "a stage word with two readings is a ruling nobody can check".
- **Ruled:** 5 September 2026. Who is not stated.
- **History:** *v1* and *Core* respelled by meaning, 5 September 2026. (l.89 still says "Core".)
- **Source:** DESIGN.md l.3

## The core vision

### Everything lives in one structure, the Logic Graph
Programs, types, proofs, compilation rules, optimization passes, the compiler, documentation, tooling, and the language's own parsing rules all live in one data structure, the **Logic Graph** (LG). No line between "the language" and "what is written in it". Everything reachable is queried and changed through the same meta-reflection (**radical unification**).
- **Why:** the usual boundaries (language/compiler, code/specification, program/proof, source/tooling) are accidents of history, not necessities. Removing them makes the core simpler, lets it state more, and makes it more honest about what it is.
- **Source:** DESIGN.md l.7, l.9

### Logos is a serious systems language
Static memory safety through a borrow checker (lexical lifetimes, no GC, explicit ownership and moves), zero-cost abstraction, capability tracking for effects, layered compilation to many backends. The unification does not cost much performance.
- **Source:** DESIGN.md l.11

### Interpreted by default; any function can be JIT compiled, mutable code included
LG code is interpreted by default. Any function can be explicitly JIT compiled with Cranelift, faster in most cases. Mutable code too: a structural edit to compiled code drops the compiled form and falls back to interpretation (›Mutable code is compilable‹).
- **Why:** whether to compile is a question of profit, never of what the code means.
- **Ruled:** 27 September 2026, Thobias: "mut functions should also be compilable".
- **History:** until 27 September 2026: "an immutable (frozen) region can be explicitly JIT compiled … whereas mutable code stays interpreted". Dropped because it disagreed with ›Mutable code is compilable‹ and with the 7 September 2026 ruling that there is no frozen state.
- **Source:** DESIGN.md l.11

### Reflection needs the LG form
Reflection cannot work below the LG. JIT compiled code stays fully reflectable through the LG it was compiled from; interpreted code *is* its LG.
- **Source:** DESIGN.md l.11

### The seed is compiled Rust that acts like frozen, JIT compiled Logos
The seed is neither interpreted nor JIT compiled, and never changed at runtime. It ships its primitives as `native` (callable machine code, opaque to reflection); each becomes reflectable only once ported to Logos source. So the seed reflects in full only at the self-hosted end; no version before that promises it.
- **Why never mutated:** it is all compiled, and "one would not want to borrow every seed identity".
- **Source:** DESIGN.md l.11

## Why now: a substrate for machine-written code

### Machines now write the code, so a self-checking structure becomes a requirement
The unification idea is old. A reflective, rewritable structure carrying its own types and proofs was a luxury while humans wrote code (part of why the closest ancestors stayed niche). When models write most code, it becomes a requirement.
- **Why:** the feedback loop. A model editing *text* pushes a guess through a fragile toolchain and hopes. A model editing the LG rewrites a structure that already carries scopes, types, borrow states and (above the seed) proofs, and gets machine-checked feedback that the change is correct and safe before it runs. The reader-writer rule that governs memory also governs self-modifying code (›Representation, elaboration, and lifetimes‹), so a program can improve its own code without being allowed to break it. A non-human author most needs machine-checked correctness and least reliably supplies it.
- **Source:** DESIGN.md l.15, l.17

### Logos aims for the union of Smalltalk and Lean, on a systems base
- **Smalltalk** is malleable but unsafe: a live, fully reflective image rewriting itself at runtime, but dynamically typed, no borrow checker, no static guarantees, no way to prove a self-modification right; reflection is runtime-only with no types, lifetimes or proofs; image-based and garbage-collected, not bare-metal.
- **Lean** is proven but not a fast, rewritable substrate: machine-checked proofs down to a small trusted kernel, self-hosted, real metaprogramming, but garbage-collected and functional (not for systems, GPU or native speed); surface syntax, elaboration and kernel term language are separate layers, not one structure the running program rewrites; its verification targets mathematicians proving theorems, not a graded floor from a borrow check up to a full proof over the same code.

Logos: rewrite as freely as Smalltalk, check as strictly as Lean, run as fast as Rust, in one structure. The how: the single reader-writer rule, graded verification strata, interpret by default with opt-in compilation.
- **Source:** DESIGN.md l.19-24

### The bet is staged and testable
Near claim: the coding feedback loop, measured by an agent evaluation (a model given the graph and its machine-checked feedback vs. the same model on an established text language) as soon as v0.1.0 exists. Far claim: agents reasoning over hosted human language with provenance; it sits behind the proof layer and hosting and never carries near-term weight.
- **Ruled:** 31 August 2026, in discussion.
- **Source:** DESIGN.md l.26

## The architecture

### The Logic Graph is the primary representation
Neither an AST (syntax only) nor a normal IR (codegen form). It holds the program with all semantic information attached (resolved scopes, inferred types, borrow states, propagated capabilities), the rules that governed its parsing and elaboration, the standard library, the compiler's own logic, and later proofs about all of it. The operations users run on their own code walk any subgraph, the compiler's included.
- **Source:** DESIGN.md l.30

### A small Rust seed starts the system
The seed holds the minimum: a parser producing LG nodes, an evaluator for them, enough type machinery to check the kernel, a path to Cranelift. Everything else (full type strata, borrow checker, rewriting engine, optimization passes, standard library) is Logos processed by the seed, until the system compiles itself. Once built the seed is compiled and immutable: readable as LG by tooling, never rewritten at runtime, the single trusted core everything is processed by and traced back to. The discipline is restraint about what belongs in it.
- **Why small:** auditable by hand, later verifiable, a fixed reference point: it makes Logos formally tractable.
- **Source:** DESIGN.md l.32

### The core is one hand-built cyclic graph, not a sequence
The core identities (the dyad cell, `type`, `Scope`, the primitives) depend on each other and come into existence together. Sketches of the core are *specifications* of that graph, not programs: text order is not evaluation order. Checking the seed's graph against them is a planned verification step.
- **Source:** DESIGN.md l.32

### What reflects is data; what is opaque is the callable, bottoming out in `native`
The line runs between callables and data, not between seed and user code. Every LG structure is reflectable (types, values, scopes, a function's source body); reading it is what the interpreter and lowering do. Opaque is the *callable*, a machine-code body (`@exec`) you invoke but cannot read into. A function is both: a reflectable source body and, once compiled or native, an opaque callable. A **`native`** function is callable only, with no Logos source.
- The seed ships every primitive as `native`, so no spec-LG is owed for them; the seed-against-spec check is deferred, not cancelled, and returns per identity as each is ported.
- Self-hosting replaces natives with Logos source one at a time; each keeps its callable. The boundary never moves; the `native` set shrinks toward a floor that can never have Logos source: `invoke`, allocation, raw memory, syscalls.
- **Why:** forced. Interpretation consults each node's evaluator; if every evaluator were LG to interpret, the recursion would never end. Some functions must be machine code, and compilation exists to produce them.
- **Source:** DESIGN.md l.34

### A dependency cycle among core identities is broken with native versions first
Never by ordering the Logos definitions. Each identity in the cycle (`array`'s chooser keeps its mints in a map; a map is built on an array) has a native version in the seed first, and the Logos definitions replace them one at a time.
- **Why:** Thobias: "likely have some version of each identity that runs in rust before defining all in LogosLang".
- **Ruled:** applied 23 September 2026, Thobias.
- **Seed:** the 23 Sept text calls `hashmap` "a native the seed owes, pending"; the 25 Sept seed note (#137, below) describes it as present.
- **Source:** DESIGN.md l.34

### A declared hashmap is the empty map: `m := hashmap K -> V`
`m` is readable at once. `m[k]` on a missing key hands back `?` or is the checked error, as the value type's tolerance for `?` decides; `m[k] = v` fills it. Example: `share array_mints := hashmap type -> type` inside `get_mint`, identities/array.logos (chooser mechanics under ›Representation, elaboration, and lifetimes‹). Chosen knowing it reads unlike `t := array i32` (leaves the mint standing as a type value, ›A type is a comptime value‹) and `x := i32` (names the type): alike spellings, different meanings, since `hashmap`'s constructor decides what its bare form gives, as tolerance for `?` is written per constructor.
- **Why:** a declared container has a known value, empty, so nothing is unknown and the read veto of ›Declarations are immutable by default‹ has nothing to guard. Thobias: "The hashmap is empty after it's first declared so there shouldn't be any problem"; he chose the bare spelling when asked.
- **Ruled:** 25 September 2026, Thobias.
- **Open (to Thobias):** is `m := hashmap K -> V ?` refused ("a hashmap is never unknown") or what the seed makes it (the same empty map, no veto)? Does the bare type want its own spelling?
- **Seed:** (#137) `hashmap K -> V` places a fresh empty map, and the interned type only where `?` follows, so `(w := hashmap K -> V ?)` stays a parameter. The type is read off a map: `m:type` or `(hashmap K -> V):type`.
- **History:** first a top-level `array_mints := …`; inside `get_mint` with the mark since 25 September 2026. The mark `shared` was renamed `share` on 26 September 2026 (l.207).
- **Source:** DESIGN.md l.34

### The Logos IR sits between the Logic Graph and every backend
Target-agnostic, with explicit basic blocks, control flow, memory operations and types; target-agnostic optimizations run here (constant folding, CSE, inlining, monomorphization). It is itself representable in the LG, as another stratum. It should fit many backends (Cranelift, LLVM, WASM, possibly unique hardware), with a way to describe what each can and cannot do, and be extensible. The v1.0.0 IR should be solid for at least most things Cranelift is solid at.
- **Source:** DESIGN.md l.36

### One backend interface
Anything taking Logos IR and emitting code (Cranelift, LLVM, a custom embedded compiler, WebAssembly, GPU) satisfies one interface. Cranelift is the default (fast compilation, good code, ideal for meta-circular execution); LLVM is selectable for production and hot loops. Backends mix per function or module through scope modifiers like `backend cranelift (...)` or `optimize_with llvm (...)`.
- **Note:** l.182 says v0.1.0 compiles through `f.compile()`; the scope-granular `backend cranelift (...)` marker is later work.
- **Source:** DESIGN.md l.38

## The rewriting engine

### One operation for optimization, algebra and user transformations
Take an LG or Logos IR fragment, apply rewrite rules, extract the form with the lowest cost. The same engine does the compiler's `x + 0 → x` and the mathematician's `sin²(θ) + cos²(θ) → 1`. Compiler optimization passes are Logos libraries over it; adding a pass is library work.
- **Source:** DESIGN.md l.42, l.51

### Equality saturation under a budget
Instead of rewriting destructively in priority order, the engine builds an e-graph of all forms equivalent under the rule set and extracts the cheapest. Extraction is optimal over what the e-graph holds, but a fixpoint is not guaranteed (size-increasing rules grow it without end), so it runs under an iteration, size and time budget and takes the best found. Optimal extraction under a sharing-aware cost is NP-hard: the default extractor is heuristic, with an exact mode.
- **Why:** production-proven: Cranelift's mid-end uses an e-graph, and production systems also bound the search.
- **Source:** DESIGN.md l.44

### Rule sets and cost functions are first-class values
A rule is a pair of LG patterns (match, rewrite); the standard library ships sets for arithmetic, algebra, trigonometry and compiler optimizations. A cost function maps LG nodes to costs; shipped: FLOP count, numerical stability, code size, register pressure, target-specific metrics. Users compose, extend, override, or supply their own. Every target is a choice of cost function and/or rule subset: minimum-cost (default `simplify`), canonical (equality testing), factored (root extraction), expanded (pattern matching), numerically stable (Horner, log-sum-exp), target-specific.
- **Source:** DESIGN.md l.46-48

### The engine can run at compile time
Because `eval` over LG fragments is built in, the engine can simplify symbolic expressions at compile time and bake only the best form into the binary. Differentiating, simplifying the derivative, and emitting native code "is one `eval`-bracketed pass with no runtime symbolic machinery". Likewise `eval (optimize_for(target, kernel))` rewrites user code for a target at compile time.
- **Source:** DESIGN.md l.49

## The type strata and verification

### Strata compose, and users pay only for what they use
Systems programmers get the base stratum and borrow checker; more guarantees mean opting into refinements and pre/post-conditions; scientific, cryptographic or verified-systems work opts into dependent types (l.65 wording; that tier is now the proof layer, see below). Parts of a program can be verified while the rest stays lower.
- **Source:** DESIGN.md l.55, l.65

### Base stratum: types, ownership, capabilities, and one borrow rule
Primitives, record types, enums, arrays, function types, references with lexical lifetimes, ownership and moves, capability annotations (`pure`, `total`, `async`, `thread_pinned`, GPU-subset, allocator-bound). The borrow checker works here.
- **One rule:** among references live at the same time that overlap, many readers XOR one writer (read∩write and write∩write empty, read∩read free).
- The checker is one generic function with one question: *can these two extents overlap; does this access fall inside this extent?* Answer: yes, no, or unknown. The tiers are answerers of rising strength for that question, not separate rules: identity and field-offset comparison; constant then symbolic index arithmetic via SMT; predicate disjointness where decidable; kernel-checked proof beyond.
- Whole values and fixed places (fields, elements): decidable, fast, errors point at nodes the user wrote. A *predicate-defined* set of indices: its disjointness goes through SMT, so it is bounded by the solver, not constant-time.
- Borrow "cases" never differ in the rule, only in how hard its inputs are: which references coexist (lifetimes, control flow, threads) and which exist at all (summaries at function boundaries, since references escape through data structures).
- **Source:** DESIGN.md l.57

### Refinement types
Predicates on primitives (`i32 where >= 0`, `i32 where 0 <= self < array_len`); an SMT solver discharges obligations at assignments and call sites. This rules out out-of-bounds, division by zero and overflow at compile time wherever the solver succeeds. Where it cannot, the obligation is an error to resolve (stronger precondition, explicit check, handled case), never a silent pass. Code without refinements is unaffected.
- **Source:** DESIGN.md l.58

### Pre- and post-conditions
Refinement predicates on inputs and outputs: preconditions proven at call sites, postconditions in bodies. A call yields its postcondition as a new fact. (See ›`fn` takes the same form‹.)
- **Source:** DESIGN.md l.59

### Termination annotations
Opt-in decreasing measures for functions that take part in proofs. Functions without them stay usable but cannot appear where totality is required.
- **Source:** DESIGN.md l.60

### Top tier: any mathematical statement about a program is statable and provable
How: proofs are rewrite rules and `proof` is its own type (›The proof layer‹).
- **History:** was "full dependent types and proof terms: types depending on values, propositions as types, proofs as programs, checked by a small trusted kernel". Superseded in discussion, August 2026: statable-and-provable stands, the propositions-as-types shape does not.
- **Source:** DESIGN.md l.61

### A rewrite rule is its own proof, so verification composes with rewriting
The rule *is* the proof; its derivation is the body the trusted core checks. So simplification provably preserves the property (equality, refinement satisfaction, numerical equivalence within tolerance). A verified CAS comes out as a library.
- **Ruled:** sharpened in discussion, August 2026.
- **History:** first "each rewrite rule can carry a proof"; August 2026: the rule does not carry a proof, it is one.
- **Source:** DESIGN.md l.63

## The proof layer

### A conjecture is statement, context, evidence
`half := conjecture ( (a + a) / a -> 2 ) where ( a:type is number and a != 0 ) proof ( … )`
- The statement holds `pattern` and `replacement` as one `->`. **Left side: what is recognized. Right side: what it becomes.** `a + a -> 2 * a` recognizes the sum and yields the product; `a + a == 2 * a -> true` recognizes the equation and yields `true`. Two recognizers, two conjectures, never one object read two ways.
- `where` holds **one boolean**, the `premises`. A hole's type is a premise, not a declaration: `a:type is number`, `is` being membership in the collection of numeric types (›Element access is `[…]`‹).
- `proof` holds the `derivation`. The `world` is computed.
- **Ruled:** in discussion, 6/7 September 2026; left/right precision and `is` 8 September 2026.
- **Rejected:** `== generic_number` (`==` on types is identity, and inheritance is rejected); `where (a := generic_number ?, …)` (a hole's type is a premise).
- **History:** `a:dyad.type` respelled `a:type` 23 September 2026. August spellings superseded by this form (see ›A proof is its own type‹).
- **Source:** DESIGN.md l.69

### The signature is a pattern: an unknown spelling in it is a hole
The same spelling is the same hole. This is the one place an unknown spelling may stand anywhere but left of `:=` (›A language is a section with an authored start‹): the regex capture group lifted to the graph.
- `conjecture` reads its own bracket, joining `fn` and `type` in the discovery-time set of ›The scope's constructor is the driver‹, because inside, an unknown spelling means a hole and names resolve in the conjecture's own scope.
- The first lex of an unknown spelling declares it: the spelling enters the trie pointing at a fresh null-slotted dyad, as `key := ?` does. Later same spellings resolve to it; nothing is unified afterwards.
- An operator over holes constructs with its op slot unresolved, the generic-fn shape (›Deferral is authored‹). `where` is stored as residual code and runs per match.
- A match is a frame whose parameters are the holes (›A function's surface‹: a call frame is an instance of its function). The holes are the conjecture's `input`, reflected like a parameter list; `a:dyad` in the frame is the matched node.
- A known spelling is a reference (a constant of the pattern), never a hole, and no error fires when an outer name shares a hole's spelling. Accepted consequence: `a := 5` before `conjecture ( a + a -> 2 * a )` leaves no holes; it recognizes `5 + 5` alone.
- **Why:** nesting needs it (`is_galois_stable := fn (N) -> conjecture ( … N … )` reads `N` as a fixed value), and the hole set is readable off the conjecture.
- **Ruled:** parse 8 September 2026.
- **Rejected:** holes bound dyad-valued (closed 8 September 2026). The name is what is called, so everything the binding holds is reached through the name by the one rule, `:` for the binding, `.` for fields; a hole reads like any name. (Said then as "`a:dyad.type` stays"; `a:type` since 23 September 2026.)
- **Source:** DESIGN.md l.69

### No proof means derivation `?`; there is no `axiom` keyword
A statement without `proof (…)` has derivation `?`, attached later through the field, `half.proof = ( … )`; the slot is writable until defined. Such a conjecture is an axiom exactly where a scope's rule set registers it (›The branch is the axiom-closure‹); cited elsewhere it enters the citing proof's world as an assumption. A root theorem is proved from the base exactly when its world holds nothing else. `theorem`, `axiom` and open are states the identifier reports (accepted derivation / registered without one / neither), not types.
- **Rejected, to stay rejected:** the `axiom` keyword.
- **Ruled:** in discussion, 6/7 September 2026.
- **Source:** DESIGN.md l.69

### A derivation is a chain of `->` steps
`->` is the *mapper*: directed, "convert this form into that", target named. A boolean operator is a *relation* and names no target, so only `->` stands between lines. Each right side holds one call of the cited conjecture at the position it applies, its argument the pattern instance, checked against the previous line's reduced form. A line with no call is an identity step showing the reduced form. The trailing form is the replacement:
`proof ( (a + a) / a -> double(a + a) / a -> mul_div_assoc((2 * a) / a) -> 2 * div_self(a / a) -> mul_one(2 * 1) -> 2 )` (`div_self`'s premise `a != 0` is `half`'s own).
- **Rejected, to stay rejected:** a relation symbol or implication symbol between lines. The mapper is what a step is; the relation it preserves is the cited rule's.
- **Ruled:** in discussion, 6/7 September 2026.
- **Source:** DESIGN.md l.69

### The relation a mapping preserves is fixed by what it maps
- Over **values**: equality. A step rewrites a subterm anywhere; accepting the conjecture merges its two sides into one equality class (the engine's *known equal*).
- Over **booleans**: goal reduction, the left implied by the right. `a <= c -> (a <= b and b <= c)` reads "a <= c reduces to those two". A truth rule `S -> true` is a chain from S down to `true`: `x <= x^2 + 1 -> 0 <= x^2 - x + 1 -> 0 <= (x - 1/2)^2 + 3/4 -> true`, each step citing an iff rule, a value rewrite inside the line, or a rule whose right side is `true`, as written by hand.
- No relation slot. "A implies B" is the reduction `B -> A`, or the truth rule `(A => B) -> true` cited through modus ponens.
- **Why:** it follows from the recognizer reading with nothing added (for a value the sides are equal; for a boolean the right side is what is left to show). Goal reduction is the direction the coherentist backward search already walks, so written and searched proofs read alike. A relation slot would add a spelling and a composition table for a case the boolean chain covers.
- **Ruled:** 8 September 2026.
- **History:** superseded 8 September 2026: the composed relation ("`==` with `<=` is `<=`", `x <= x^2 + 1` proved by mapping `x` to `x^2 + 1`) and "steps cite truth rules only: a value rule is the reading of an accepted equation, never a second object".
- **Source:** DESIGN.md l.69

### Implication lives in two places, neither the chain
As a boolean operator: `not a or b`, no proof machinery, like `==`. In a proof: a premise, discharged at the application site.
- **Source:** DESIGN.md l.69

### A conjecture is callable
The behavioural merge with `fn`. The argument is the shape, holes bind by matching, premises are checked at the site: a call whose premise cannot be shown true fails, never silently succeeds. It returns the replacement (`true` for a truth rule). Explicit call and engine matching are two roads to one core-checked step. A normalizer (›A type with a canonical form‹) is a type taking the second road for its own theory. A `( ? )` derivation with a world and budget asks the engine to find the calls.
- **Source:** DESIGN.md l.69

### Quantification nests by returning a conjecture
`∀ x : T, P(x)` is the rule form itself. A named proposition is a `fn` returning one: `is_galois_stable := fn (N) -> conjecture ( … )`. So a quantified hypothesis enters as a rule-valued premise (as induction already licenses). Connectives are library types with proof fields: two proofs for conjunction, a witness beside its evidence for existence. Proof-carrying data is "a field, not a mechanism".
- **Source:** DESIGN.md l.69

### `and`/`or` on non-booleans build a group; every operator on a group applies to each member
`(a and b and c):type == i32` reads each member and collapses with `and` at the first boolean. Before collapse it is not a boolean, so `if a and b` on two numbers stays an error. Reads, comparisons, arithmetic and calls all apply per member: `(a and b) + 1 == 3` is `a + 1 == 3 and b + 1 == 3`.
- **Why one rule, no operator list:** a group is "these, each"; collapse at the first boolean bounds the shape; a list would be a second rule to hold. Broadcast arithmetic on a group is the accepted consequence.
- **Ruled:** 8 September 2026.
- **Source:** DESIGN.md l.69

### A fact about two distant nodes uses two holes
`where` relates them by navigation; or the relation is first made a node or binding entry and proved locally (as the drop model's constructor-inserted `defer` links an allocation to its teardown, and the identity binding's `start`/`end` link a declaration to its `own` or `drop`). E-matching matches across known-equal nodes. Not covered: state between two points ("x is 5 when line 40 runs"); it needs the operational model as a rule set and stays the flagged hard part.
- **Open:** the navigation words `where` needs for this (a graph range `w..r` in program order, `in`, `for x in w..r` over a boolean body), as the first meta-access vocabulary (›Meta-navigation‹).
- **Source:** DESIGN.md l.69

### `fn` takes the same form
`f := fn (a, b) -> c where ( (a and b and c):type == i32 ) ( … )`. The signature is a pattern. `-> c` names the result hole, so `where` states postconditions (`c >= a`): the pre/post tier of ›A function's surface‹ with no new mechanism. The body writes `c` by name as often as it likes (a mutable place: `c = …`, never `c := …`); a bare `return` leaves with whatever `c` holds. Short form `fn (a := i32 ?, b := i32 ?) -> i32 ( … )`: `i32` alone is the identity, matching it the premise `a:type == i32`; with an unnamed result the trailing expression is the value and `return X` the early exit (›A scope's value is what it evaluates to‹).
- **Ruled:** 8 September 2026.
- **Rejected:** implicit "trailing expression fills `c`": a result with a name is written by its name.
- **Source:** DESIGN.md l.69

### `where` is checked where it can be
What the pass decides (type premises, premises over comptime values) is checked there; the rest becomes a residual each call checks at run time. In v0.1.0 a `where` may hold only what the pass decides; a run-time residual is the checked error until the refinement tier lands.
- **Ruled:** 8 September 2026.
- **History:** "fn a type, proof a shape" superseded: both arrows are followed by a shape, a bare type being the shape "a value of that type".
- **Source:** DESIGN.md l.69

### A proof is its own type, merged with `fn` in behaviour, never in structure
Both bind typed identities, carry bodies and share `->`; each type's constructor decides what may follow its arrow. A proof also stores `pattern`, `replacement`, `premises`, `derivation`, and its **world** (the axiom set it rests on). Being an ordinary type makes it an ordinary *value*: a sorted array can hold beside its bytes the proof of its ordering. Premises are extra inputs that must rewrite to true at the site (implication); typed holes are quantification; `==` and kin stay plain boolean operators.
- **Ruled:** in discussion, August 2026; respelled 6/7 September 2026 by the conjecture form, this substance standing.
- **Rejected, to stay rejected:** usage verbs on proposition types (`forward`/`backward` on `==`); the proposition-as-output-type spelling `fn (a) -> (a + a == 2 * a)`.
- **History:** superseded propositions-as-types (August 2026). Superseded August spellings: `p1 := proof (a := generic_number ?, a + a) -> (2 * a) ( … )`, `proof (a := generic_number ?, a + a == 2 * a) -> true ( … )`, `proof (a := generic_number ?, a != 0, a / a) -> 1`.
- **Source:** DESIGN.md l.71

### Truth rules and value rules
A **truth rule** states any proposition whatever.

CONFLICT: l.71 (August 2026, not marked superseded): "The truth rule is the primitive: a value rule is accepted exactly when its truth counterpart holds." l.69 (6/8 September 2026): "`a + a -> 2 * a` recognizes the sum and yields the product where `a + a == 2 * a -> true` recognizes the whole equation and yields `true`: two recognizers, two conjectures, never one object read two ways"; "a mapping over **values** preserves equality … accepting the conjecture merges its two sides into one equality class"; and it supersedes "steps cite truth rules only: a value rule is the reading of an accepted equation, never a second object". Unclear whether a value rule is still accepted only through its truth counterpart.
- **Source:** DESIGN.md l.69, l.71

### The trusted core checks derivations and demands totality
Checking the chain (every step cites an already-accepted proof or axiom) is the core's whole job. Only the core may accept a body as evidence. It demands totality: a non-terminating body would prove anything.
- **Source:** DESIGN.md l.71

### Induction is a higher-order axiom, minted per datatype
An axiom is asserted, never derived. Induction is one: higher-order (one premise is itself a rule), mintable per datatype by the type's own constructor, the same shape as a constructor writing its own teardown.
- **Source:** DESIGN.md l.71

### Application is the rewriting engine unchanged; matching is fail-closed
Matching binds holes (calling in reverse). A non-linear pattern like `a + a` needs both slots to hold the same `a`: *known equal*, meaning same content or proven equal by the rules applied so far (the engine's equality class); with no engine running, the same dyad by address. Fail-closed; the binding is checked against the hole's declared type. The engine applies rules non-destructively under equality saturation, extracting by cost under budget, so registered directions never ping-pong and a goal not reached in budget is not proven.
- **History:** wording amended in discussion 30 August 2026: "by address" is the case with nothing yet proven.
- **Source:** DESIGN.md l.71

### Rule sets as values carry the ecumenical system over unchanged
A scope (section, sub-language, import) registers the rule set it accepts; an axiom's acceptance is its presence there. A proof's `world` is the union of the worlds of the steps it cites. Two results combine only where their worlds' union stays consistent (›The ecumenical proof system‹), with the coherentist layer ranking the unproven.
- **Source:** DESIGN.md l.71

### Proofs are a layer over the Logic Graph, not a property of the substrate
The node model classifies a value by its `type` pointer, ending in the self-referential `type : type`. That loop is sound as *operational* machinery (never read as a logic) but inconsistent as a logic (Girard's paradox), so the proof system does not build on it. A proof is an ordinary, distinguishable, reflectable value; no type of types is involved; erasure is an optimization. Stratification returns only if rules quantifying over rules (induction as a higher-order axiom) are shown to admit a paradox.
- **Ruled:** 30 August 2026, in discussion.
- **History:** superseded 30 August 2026, because proofs as rewrite rules involve no type of types: the `universe_of` classification stored per node beside `type_of` (the Lean/Coq universe treatment); the predicative tower `Universe 0 : Universe 1 : …`; `Prop` as impredicative, proof-irrelevant, erased before codegen; connectives (`False`, `And`, `Or`, implication, `Forall`, `Exists`, equality) as inductive and function types; Curry-Howard at the representation level; kernel obligations (checking stored universes, strict positivity, definitional equality only on the terminating tagged fragment, no elimination from `type : type` into `Prop`). The June 2026 open question (proof-irrelevant `Prop` needing the large-elimination restriction, or proof-relevance) is closed by the proof-relevant reading above.
- **Source:** DESIGN.md l.73-81

### The sound layer can reason about any node, but not its own consistency
It can prove properties of *any* LG node (the substrate and the proof system itself included) by reasoning over representations and an operational model, treating substrate types as syntax and never lifting substrate classification into logical truth (the CompCert/MetaCoq discipline). It verifies everything above it. By Gödel's second theorem it cannot prove its own consistency; that part is trusted by hand-audit.
- **History:** the same paragraph defined a theorem as "a definition whose type is a proposition and whose value is a kernel-checked term" and an axiom as "a proposition declared with an empty value slot": propositions-as-types, superseded August 2026 (l.71). Now theorem/axiom/open are states (›No proof means derivation `?`‹).
- **Source:** DESIGN.md l.83

### The proof layer ships in v1.0.0's standard library, not in the seed
The substrate runs without it, so it ships after the base language, in the standard library, **inside v1.0.0**: refinements, the rewrite-rule proof layer, its trusted core, the ecumenical system. The proof kernel is an ordinary importable library, trusted only by code that opts in, so it never enlarges the seed's mandatory trusted base (still the single core everything is processed by). Everything on top (connectives, theorems, tactics) is *checked* by it, not trusted.
- **Why v1.0.0:** the release is the completeness promise (›Completeness, not cleanliness‹); leaving verification out would leave exactly the hole downstream fills differently.
- **Ruled:** 5 September 2026.
- **History:** ›Feasibility‹'s "not needed for an initial release", superseded 5 September 2026.
- **Source:** DESIGN.md l.85

## The ecumenical proof system

### Status
Direction reached June 2026. Deferrable like the rest of the proof layer, and inside v1.0.0's standard library (l.85, 5 September 2026). l.89 still says "ships after Core" (*Core* respelled 5 September 2026, l.3).
- **Ruled:** direction, not ruled, June 2026.
- **Source:** DESIGN.md l.89

### Proofs live in a graph of theories
One consistent axiom set is one *branch*. The LG holds many, even contradictory ones (`P` here, `¬P` there), each internally consistent, not poisoning each other. Contradiction comes only from combining incompatible assumptions in *one* proof, which the system never does: every proof carries the **set of axioms it depends on**, and two results combine only when the union is consistent. Named after the proof-theoretic *ecumenical systems* where classical and intuitionistic logic coexist; structurally a *theory graph* (cf. MMT, Hets, IMPS "little theories") carrying its own dependency provenance.
- **Source:** DESIGN.md l.91

### The branch is the axiom-closure, not the whole dependency-closure
Definitions are branch-neutral and shared; only axioms commit a proof to a branch. Checking a proof emits the axioms it transitively rests on (like Lean's `#print axioms`); that set is its world. A branch is added *soundly* only by forking on an **independent** statement, one the base can neither prove nor refute (the parallel postulate over Euclid's other axioms, CH over ZFC): assume `P` or assume `¬P`, never prove both from one base (that would make the base inconsistent).
- **Note:** l.93 still calls axioms "propositions asserted with an empty value slot"; today an axiom is a proofless conjecture a scope's rule set registers (l.69).
- **Source:** DESIGN.md l.93

### Consistency is checked, never guaranteed
Two axioms that look independent may be jointly inconsistent, deriving `False` only through some chain, and finding it is undecidable (Gödel's second theorem). So a combination counts as compatible **until a `False` is derived**; then the cluster is marked poisoned and its results withdrawn (non-monotonic, like assumption-based truth maintenance and belief revision). "Consistent" means "no contradiction found within budget", never "proven impossible".
- **Source:** DESIGN.md l.95

### A read-only coherentist layer ranks plausibility
Backward search reduces a goal to the sub-statements that would establish it, recursively. Bottoming out at axioms or proven theorems: **proven**. At a web of unproven but densely connected statements: **plausible** (a ranking). The more consistent structure depends on a candidate axiom and the more it interconnects, the higher its plausibility. Scores decide *what to try proving*; they **never** count as proofs (kernel → ranking, never ranking → kernel).
- **Why:** humility: the system corrects itself the instant a proof or contradiction arrives, keeping every still-consistent branch alive.
- **Source:** DESIGN.md l.97

### Branches and universes

CONFLICT: l.99 (June 2026 direction, not marked superseded): "It composes with the universe kernel rather than replacing it. The two are orthogonal layers over the one graph: *universes* stop self-referential paradox **within** a theory (the stratified `universe_of` tower above); *branches* manage incompatible assumptions **across** theories. They meet at one productive point: large-cardinal axioms, which assert that taller universes exist, so there 'which branch am I in?' and 'how tall is the universe tower?' are the same choice, settled (as in real mathematics) by fruitfulness, i.e. by the coherentist layer." l.75 (ruled 30 August 2026): "the `universe_of` tower, `Prop` as an erased proof-irrelevant universe, and the June proof-relevance question below are no longer live spec … Stratification re-enters only if rules quantifying over rules … is shown to admit a paradox, not before."
- **Source:** DESIGN.md l.99, l.75

## Memory and concurrency

### Locals live on the stack; there is no garbage collector
Locals belong to their scope, and their memory comes back with the frame in one bulk step with no side effects. Giving memory back and tearing a value down are different: giving back memory that needs no teardown is implicit; running teardown never is, it is always visible `defer` structure.
- **Source:** DESIGN.md l.103

### The heap is explicit: `alloc n` and `alloc n of T v`
`alloc n` allocates n bytes and returns `@u8`. `alloc n of T v` allocates n cells of `T` in one unbroken span, each built from the constructed value `T v` (`T ?` leaves them unfilled), and returns `@T` to the first cell. The count is always written: one value is `alloc 1 of i32 5`; there is no form without a count. Cell k is `(p + k)@`.
- **Why:** Thobias: "one spelling, no hidden default; the count is the caller's business".
- **Ruled:** 8 September 2026, Thobias; recorded 23 September 2026.
- **Seed:** since 23 September 2026 the span's byte count sits in a header before the first cell, and `free` reads it back.
- **History:** first spelled `alloc T v`, returning an owning `@T`.
- **Source:** DESIGN.md l.104

### Nothing is destroyed implicitly: the constructor writes the teardown as `defer`
No hidden end-of-scope rule frees anything. The constructor writes the teardown and inserts it as `defer` structure that reflection can read. `alloc` inserts `defer free a`, naming the *specific allocator*, into the scope where ownership of the result lands. Any constructor may do the same (a file-opening constructor inserts `defer close`). `defer` runs last-in-first-out at scope exit, so teardown reverses construction order. Since teardown is ordinary graph structure, not hidden "drop glue", "every alloc reaches a paired free on every path" can be proved over the graph by the proof layer: RAII as checkable structure, not a trusted builtin.
- **Why:** only the construction site knows how a value was built: which allocator, whether ownership was taken, which invariants hold. An `@T`'s type cannot carry that, since ownership does not always follow the name holder (sometimes the pointee must outlive the name). Naming the allocator in the call is what lets many allocators coexist safely. Teardown has too many shapes for one mechanism; each constructor knows its own.
- **Ruled:** settled in discussion, July 2026.
- **Rejected, to stay rejected:** implicit scope-end destruction in any form; `:=` inserting `defer drop` on every declaration. A declaration stays neutral; only constructors write teardown.
- **Source:** DESIGN.md l.104

### A type whose fields carry teardowns must write its own destructor
Defining such a type without a destructor is a checked error at the type definition, pointing at the fields. There is no derived last-in-first-out fallback: the order is written per type, and the destructor's lines are the per-field teardown items (which a partial move can make the scope exit skip, see ›`own` and `drop` take a field path too‹). A destructor gets its value by **mutable reference**, never by `own`, and tears fields down in place: `free` and `close` on each resource, and a nested field's destructor called as the ordinary function it is.
- **Why (mutable reference):** an `own` parameter is a binding site, and the binding-site rule would attach to it the very teardown the destructor is.
- **Ruled:** 30 August 2026 (order per type). Mutable reference: from the 3 September 2026 ruling, still standing.
- **Seed:** composing field teardowns waits on the destructor-authoring check.
- **History:** at first a record type's auto-derived constructor composed its fields' teardowns; per-type order 30 August 2026.
- **Source:** DESIGN.md l.104

### `drop` and `own` end a name; the teardown is removed or moved at parse; no drop flag
`drop x` runs an identity's destructor. `drop` and `own` both read the value and end the name, leaving the source `undefined`. The inserted teardown is then **removed or moved at parse**: an early `drop` already did it; `own` moves it to the taker's binding site (and removes the source identity); a move inside an `if` moves it into the arms that do not move. So no teardown runs over an emptied place, there is no run-time drop flag, and compiled code checks nothing (Rust keeps a flag for the conditional case).
- **Why:** with the tag bits gone the flag had no clean home, and a lifetime should be a fact the graph states, not a byte the run looks up.
- **Ruled:** 7 September 2026.
- **Seed:** stand-in: a place is emptied by writing a **null pointer**, and the scope-exit teardown does nothing on null, a run-time check the ruled model lacks; removing it is pending. `alloc`/`own`/`drop`/`free`/`defer` exist; the teardown runs last-in-first-out at scope exit as body structure. The owning heap pointer is the first identity with a non-null `drop` slot; a `&x` borrow mints the same `@T` with a null destructor, so owning-ness rides on the node `alloc` built, not on `@T`.
- **History:** first a sanctioned no-op over a null drop flag, "all with no graph edit"; replaced 7 September 2026. `own` as an access kind: gone 7 September 2026. The null stand-in, binding-site attachment and the `share` direction: settled in discussion, July 2026.
- **Source:** DESIGN.md l.104

### Teardown attaches at the binding site, parameters included
`a := alloc …` and `b := own a` insert `defer free <place>` into that place's scope. A constructor result passed straight as an argument, `f(alloc 1 of i32 5)`, is bound to the parameter in the callee's frame, and its teardown attaches there; no `own` gate is needed, since no caller place is emptied. A value that reaches no name at all is a checked error.
- **Ruled:** binding site, July 2026; parameters, 30 August 2026.
- **Seed:** covers only *named* owning bindings. A bare owning temporary passed as an argument is rejected: a bug against the ruling.
- **Source:** DESIGN.md l.104

### Three fail-closed ownership rules, and `-> own @T`
Each guards a place where ownership would escape the machinery that frees it:
1. An owning value must be bound to a name (else nothing carries its teardown).
2. A scope's value may not be a place the scope owns: the teardown would free it on the way out and hand back freed memory; `own` is how ownership leaves a scope.
3. Ownership may not cross a function return: a block hands ownership to its binder in plain view of the parse, but a call hides its body behind a return type that cannot yet say it transfers ownership, so the caller would not know it owes a `free`.

Rules 2 and 3 no longer apply to a *last* value since 25 September 2026 (›A last value moves out‹). `-> own @T` hands ownership to the caller, whose binding site attaches the teardown (`own`/`drop` as gates on a reference, the same primitive as `pub`/`mut`).
- **Ruled:** rules, July 2026; `-> own @T`, 30 August 2026.
- **History:** "ownership may not cross a function return at all" and "a bare `-> @T` returning an owned place stays the checked error": superseded for a last value 25 September 2026.
- **Source:** DESIGN.md l.104

### `&T` / `&mut T` are checked statically
Many shared OR one exclusive, with lexical lifetimes to keep the model predictable.
- **Source:** DESIGN.md l.104

### A pointer steps by whole cells
`p + k` and `p - k` on an `@T` and an integer give the `@T` k cells away (k times the width of `T`), so `(p + k)@` is cell k of an `alloc n of T v` span.
- **Why:** an array written in Logos walks its cells this way, and only a whole-cell step makes `p + 1` the next value of the same type.
- **Ruled:** 23 September 2026, Thobias: "pointer arithmetics should work though".
- **Open:** direction, not ruled: the borrow-checked tier should refuse it, Thobias: "in the future safe scope it should likely not be allowed".
- **Seed:** since 25 September 2026 (#137): pointer on the left, integer on the right, k scaled to bytes as an `i64` product in the graph, both tiers. `k + p`, `p - q` and every other pointer operator stay refused.
- **Source:** DESIGN.md l.104

### A filled `share drop` is the constructor's teardown; one name owns each value
Binding a value just made by a type whose body fills `share drop = (…)`, e.g. `a := array i32 (1, 2)`, makes the name its **owner** and inserts `defer drop a` where the ownership lands (where `alloc` inserts `defer free a`). The drop body names the instance's fields directly (›There is no `this`‹, l.207). `b := a` and `f(a)` **borrow**: no teardown for `b` or the parameter, so nothing is freed twice. `own a` moves ownership. `=` into an owner takes only a value just made or moved, as an owning `@T` place does. `drop a` runs the drop now and ends `a`.
- **Why:** an array is then freed with nothing written for it; existing code keeps working; and it is `alloc`'s model plus one rule (the last value moves out), which a block handing its value to its binder already follows.
- **Ruled:** 25 September 2026, Thobias (second of three options).
- **Open:** known hole, not fixed: until the borrow checker (#35) a borrow can outlive its owner: `b := a` then `a`'s scope ends; or `p := alloc 1 of i32 5, q := p, drop p, q@` reads freed memory.
- **Seed:** since 25 September 2026. The owner's binding carries `own` in its gate set, read by `a:gate` and consulted by `own a` and `drop a` (Claude's choice, open to Thobias: the gate set is where a name's other facts live, and it outlasts one REPL line). A move writes the null stand-in, and `free ptr` in the drop empties the field; so a borrow's `b[0]` after the owner's drop is the checked null-pointer error, while `b[1]` steps a cell past the null and faults (the hole in full). Not freed yet: a value that moves out and reaches no name (`mk()` as a statement); a value just made passed straight to a call (`f(array i32 (1, 2))`). A value a type's own `parse` places as the node itself, not a call on it, gets no owner (text: "places as `this` itself"). `own` on a parameter, which would consume the argument, is not built: checked error meanwhile.
- **History:** text spells the mark `shared drop`: renamed `share` 26 September 2026 (l.207). Text says "fields block fills": the fields block was removed 25 September 2026 (l.203); members stand in the type body. Text uses `this.ptr` and "`this` bound to the instance": `this` removed 26 September 2026 (l.207).
- **Source:** DESIGN.md l.104

### A last value moves out
A function's or block's last value, when it is a place that scope owns, moves to whoever takes the value, an `alloc`'s owning `@T` included. `mk := fn () -> t ( a := array i32 (…), a )` and `-> array i32 ( array i32 (1, 2) )` hand their array to the caller, whose name becomes the owner. A `return` of an owned place before the last line stays the checked error; the teardowns of the scopes it leaves run on the way out.
- **Why:** the caller knows it owes the teardown, because the parse sees the callee's body at the call.
- **Ruled:** 25 September 2026, Thobias.
- **Source:** DESIGN.md l.104

### A field may be `own @T ?`
The field owns what it points to, so `free ptr` in the type's `share drop` is the owner's free. An `own` field with no `share drop` is the checked error of ›A type whose fields carry teardowns must write its own destructor‹.
- **Why:** the instance owns its elements' memory, and its drop frees it.
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** `own` in a type takes a pointer hole on a field or a name: `mut a := own @i32 ?` frees what is written into it.
- **Source:** DESIGN.md l.104

### A field may be `own t ?`, `t` a type whose body fills `share drop`
`mut items := own t ?` with `t := array i32` makes the field own the node written into it; `own` is a gate on the field's binding, as on an owning name's. `items = array i32 [1, 2]` or `items = own x` fills it (in a parse, through the cell: `tape[0].items = …`). A bare name is refused, "what is assigned must own too", since field and name would both drop one array. `drop items` in `share drop` runs `t`'s drop and empties the field, so a later drop finds nothing. `own b.items` moves the node out and leaves the null, so the bag's drop skips it. Such a field with no `share drop` is the same checked error as for `own @T ?`. Without `own`, a field of such a type borrows: nothing drops it with the instance. A name works alike: `mut a := own t ?` owns what is written into it, `defer drop a` inserted at the declaration. `own` on a hole of any other non-pointer type, `own i32 ?`, stays the checked error. In an array each value is freed once: `array bag […]`'s drop drops each cell (`drop p@` runs the element's drop), each `bag` drops its field, the field's array frees its block.
- **Why:** the instance owns the node its field holds just as it owns the block behind `own @T ?`, so the same word and destructor rule apply. A type with no teardown gives an owner nothing to run.
- **Ruled:** Claude's reading 25 September 2026, asked by Thobias to "fix this"; open to Thobias. Derived from "one name owns each value" and "`=` into an owner takes only a value just made or moved".
- **Open (for Thobias):** `=` into an owning field that already holds a node does not drop the displaced node, so it leaks (as with a filled `own @T` place: "the displaced block is not freed yet"). A value just made written into a borrowing field reaches no owner and is not freed; the 30 August 2026 rule "a value that reaches no name at all stays the checked error" would refuse it once a field counts as no name.
- **Seed:** since 25 September 2026, no array-specific code; `drop items` and `own b.items` run interpreted like every field read. Closed the note "`own array i32 ?` is not in the seed yet".
- **History:** text writes `this.items` and "fields block"; see renames above.
- **Source:** DESIGN.md l.104

### Shared ownership through `share` (direction)
A `share` operator co-owns *without* emptying the source; the owning type's destructor lowers a reference count and frees at zero. Same destructor-slot plus inserted-`defer` machinery, so `own` (move) and `share` (co-own) coexist like a move and an `Rc`/`Arc` clone. Flavors: a cheap non-atomic count, whose gate denies crossing a thread boundary, and an atomic count that crosses freely; the user picks per case, so neither cost is forced.
- **Ruled:** direction, not built (July 2026); flavors settled in discussion, August 2026.
- **Source:** DESIGN.md l.104

### `own` and `drop` are static: the parse marks the name dead
Three rules, needing nothing the parse has not already seen:
1. *Same level:* `own x` / `drop x` beside `x`'s declaration makes `x` dead from that line. A move in an `if` condition runs on every path and counts here.
2. *Nested block:* `own x` inside an `if` body or inner scope, `x` from the outer scope, ends `x`'s life **at that item, on every path**. The moving arm moves it; every other path gets `x`'s pending teardown at its end (an `if` without `else` gains a hidden arm holding only the teardown: the defer node is relocated, ›Metadata has three homes‹). After the item `x` is dead, so a later `x := …` gets a fresh place. There is no "maybe moved"; nothing is looked up at scope exit.
3. *Bodies that run again or later:* `own x` / `drop x` on an outer name inside a loop or `fn` body is a checked error: the loop would read a dead name next pass, and a function may own only what it got through its parameters.
- **Ruled:** 3 September 2026; "every path" 7 September 2026; `drop` added to rule 3 the same day (it empties the place as `own` does).
- **History:** maybe-moved reading ("the phase bit still decides at run time whether the teardown fires"): superseded 7 September 2026, since every legal `own` ends a lifetime at a point the parse can name.
- **Source:** DESIGN.md l.104

### A call is a use of every outer name the callee's body reads
A body resolves outside names when parsed, so the function knows which outer names it reads, and calling it uses each of them there. `climb()` after `drop n`, where `climb` reads `n`, is the dead-name error at the call, even after `n := …` redeclares the spelling: the body's `n` is the old, ended name. To read the new name, declare the function again after it. Calls count too: `g()`, whose body calls `climb()`, is refused as `climb()` is. This assumes what the seed does: a top-level name read in a body is the shared place itself, not a copy taken at declaration.
- **Why:** Thobias: a dropped name's place may hold anything, so a body that reads it must not run. The list is the function's own, filled by the parse it already makes, so no per-name list of users (declined in ›Meta-navigation‹). The check is a range lookup per outer name per call, at elaboration; running code checks nothing.
- **Ruled:** 15 September 2026.
- **Open:** the 8 September 2026 capture rule (inner function reading an outer function's locals) still lacks wording.
- **Seed:** since 15 September 2026 (#125): the list is the fn value's trailing `outer` slot, filled by the body's parse (identities it dispatches, operands it takes, once each), read wherever a node that runs a body comes to exist: a call, a node of a `run`-carrying type (applied or built by its constructor), a Logos-written constructor's run. A name from a section on no caller's stack counts as live: an imported `pub` function reads its private siblings after the section's parse ended, and importers share the one loaded scope (›Importing is dropping the text there‹). A scope that the code asking for a body stands in (a held `type (…)` built when its function runs, a run body built for a field-type set) counts as open while that body is built, since that code runs inside it: Claude's fix 25 September 2026, open to Thobias (`array bag` built the mint's held body in array.logos's scopes alone, so `bag`, whose `parse` calls a `fill` reading the command line's `array`, was refused "`array` is not in scope here"). Callee outer names join the caller's list.
- **Source:** DESIGN.md l.104

### A dead name takes nothing until `:=` redeclares it
No read, write or pass (›Name resolution is scope-filtered‹).
- **Rejected, to stay rejected:** refilling a moved-from `mut` place with `=`. Rust re-initializes such a binding; Logos redeclares.
- **Source:** DESIGN.md l.104

### `drop x` works on any identity
Runs the destructor where one is set, empties the place either way: one verb releases a name whatever its type.
- **Ruled:** inside the 3 September 2026 passage (no separate date).
- **Seed:** realized the same day.
- **Source:** DESIGN.md l.104

### An `own` argument is consumed at the call; a callee that does not take it hands it back in its error value
The caller binds the returned value to a new name (›Case study: RCU‹).
- **History:** "on failure ownership stays with the caller": superseded 3 September 2026, since `own` is static and the name is dead from the call. In v0.1.0 a failed call is a fault, so the case does not arise.
- **Source:** DESIGN.md l.104, l.113

### `own` and `drop` take a field path too
`own p.f` ends the life of the path `p.f` at that line, as `own x` ends `x`'s: a **sub-range under p's entry** in the name index, made only when a partial move happens, so it costs nothing otherwise. `drop p.f` runs f's destructor and ends the path the same way. Sibling paths stay live (`p.g` reads on), but **p is dead as a whole**: `g(p)`, `own p` and reading p whole are parse errors until scope end, because a callee holds no record that f is gone. At scope exit p's teardown **skips f**: the destructor's lines are per-field items in the type's written order; an item whose field ended in the scope is skipped, and if it ended inside an `if`, the item moves into the non-moving arms as a bare name's teardown does. So the destructor looks nothing up and no type writes an "empty" value. The three static rules apply to paths as to names.
- **Why:** lifetimes and gates live on the binding, so a field's end is one more entry of a kind the binding already holds; the 3 September 2026 cost (a per-field record of moves, a flag in the bytes) becomes a sub-range made on deviation and nothing at run time. Pulling one resource out of a value so it outlives the value was the swap door's job; this makes it ordinary.
- **Ruled:** 5 September 2026; relocation in `if` 7 September 2026.
- **Seed:** accepts `own p.f` and writes the null; lacks the sub-range, the dead-as-whole mark and the relocation: pending work, no longer a bug.
- **History:** 3 September 2026: bare names only (a value owned whole or not at all; `own p.x` a parse error), because a field is bytes at an offset, not a dyad, with no phase bits for a moved flag. Superseded 5 September 2026, and the swap door with it. "Guarded by the field's phase bit where it does not": superseded 7 September 2026. Closed 7 September 2026: how a non-pointer field carries a phase bit (none does; the teardown moves instead).
- **Source:** DESIGN.md l.104

### Place-granular borrows
Borrows are tracked per *place* (a field, an element, a predicate-defined set of indices), not per object. The borrow rule applies only where two places provably overlap: a disjointness obligation the checker (itself a Logos program, free to ask the solver) discharges through the refinement/SMT layer. So exclusive and shared borrows of disjoint parts coexist (odd indices writable, even ones shared-readable): split borrows other checkers reach only through `unsafe`. These borrows are *local*: resolved at the borrow site, in the scope that sees the place, and they do not outlive it carrying their extent. The escaping case is the next rule.
- **Source:** DESIGN.md l.105

### Reference-granular permissions (after v1.0.0; v1.0.0 must not block them)
A borrow's fine extent rides on the *reference identity itself* (a borrow is already a node), as a total, pure predicate over places. Its obligations (disjointness between coexisting references, containment of each access) discharge statically through the tiered ladder; predicates and proofs erase before codegen like all of `Prop`, so a discharged reference compiles to a bare address. A runtime check is only an explicit opt-in fallback, never silently inserted. v1.0.0 owes this future: references are graph identities at check time and raw addresses in codegen; the extent descriptor is deviation storage (the default whole-place reference stores nothing); the checker is built around the one overlap/containment query, so later tiers plug into its *unknown* bucket; places are graph values a predicate can mention; the optimizer keys aliasing and immutability on a reference's extent, never implicitly on whole objects; extent predicates face the same obligations as recognizers: total, pure, terminating.
- **Source:** DESIGN.md l.106

### Read and write are one mechanism across the system
Visibility (`pub`), mutability and borrowing (`&` / `&mut`), and graph access (reflection, `graph_mut`) are one primitive: a read or write *capability over a place, for a lifetime, granted to a scope*. Visibility is the `'static` read of a name; a borrow is the region-lifetime read or write of a value or sub-place; reflection and graph mutation are the read and write of a node. Many shared XOR one exclusive is the reader-writer rule, and it governs all of them, across threads too. The scope tree gives the default grants (a declaration is reachable in its scope and below), so the common case needs no annotation and `pub`/`mut` are sugar. Two timings: static checking for *reading* fixed structure, incremental re-validation for *changing* it, since writing the graph changes what the checker reads. The checker has a privileged read of the whole graph (it must see every node to check what is built on it), so visibility limits *peer* access only: a private field is hidden from other code, not from the checker.
- **Source:** DESIGN.md l.107

### Gates are fail-closed predicates returning `true | false | ?`
Beyond the scope tree's coarse grants, access may be set by *functions*: total, pure predicates over places or scopes (these indices writable, that field readable only by its type's methods). These are **gates**, ordinary identities; `pub` and `mut` are gates. Only `true` permits; `false` and `?` (the universal unknown) deny, and `?` stays as a standing obligation: the goal a later prover discharges by proving the access safe. Composition is Kleene three-valued, so an unresolved `?` stays visible; only the enforcement boundary turns `?` into deny.
- **Source:** DESIGN.md l.107

### A name's gates are one ordered set on its binding, combined as grants and vetoes
The set, `a:gate`, runs in declaration order; each entry answers `true`, `false` or `?` (silence). Access is permitted exactly when some entry says `true` and none says `false`, so all-silence is the fail-closed deny. Every declared name starts with one default entry: read for its own scope and below. `pub` says `true` to a read from any scope; `mut` says `true` to a write from anywhere the name is visible; `lock` seals the set, itself included, saying `false` to a write at `:gate`. A name without `mut` is written nowhere: `x := i32 ?` without `mut` can never be filled. The set's reads are membership and position; `a:mut` and `a:pub` are not spellings, since gates are entries, not binding fields. A gate word on a constructed node is the checked error: a gate needs a binding.
- **Why:** an entry that had to vote on every access would need to know every access, and "the last entry decides" would make order meaningful where it is only cost. Order still matters because a gate may read the call stack, use a budget or record the access, so two gates are not interchangeable; the identifier may reorder only gates it admitted as effect-free (›Admission is a proof obligation‹), cheapest first. No default write, Thobias: "it should not be writable at all". `mut` unscoped, Thobias: "mut just says its mutable, not where its mutable, while pub makes it public": one word, one fact; visibility is `pub`'s, and confining writes to a region is another gate's job.
- **Ruled:** 7 September 2026 (ordered set, grants and vetoes); 20 September 2026, Thobias (no default write, `mut` unscoped).
- **Open:** a gate in the `immut` family permitting a write only while the value is `?`: direction, not ruled.
- **Seed:** `mut` since 20 September 2026; the gate set is an array on the binding; the declare node's gate slot is gone. Writes through a field path or a dereference are not gated yet.
- **History:** 7 September 2026 morning: "all of which must pass"; grants and vetoes the same day. Two default entries at first, write "while the slot is null" for the own scope and below; default write removed 20 September 2026. `mut` first allowed writes from the own scope only ("a `pub` name stays writable only where it was declared"); unscoped 20 September 2026.
- **Source:** DESIGN.md l.107, l.108

### A gate is a node of the body; the binding's set is a cache folded from those nodes
`pub mut x := 5` stands in the graph as `pub` and `mut` nodes over x's declaration; `lock x` and `immut x` are later items referring to x. Reflection reads them like a declaring or emptying node: the truth is on the declaration, the binding is the derived fast path (as `start`/`end` relate to their nodes). Making a gate is a **write at each target's `:gate`**, voted on as grants and vetoes by the gates already there: the defaults admit it from the own scope and below, `lock` refuses it. So a name's first gate governs which may follow, and deleting the line deletes the gate, as deleting a `drop` line resets a lifetime. Body order is set order. **One name per gate node**: sealing y is a second line; a rule for several names is one gate identity applied per name. **Targets are names**: a gate over part of a value is a gate on its name with a condition on the path; `f(mut i32 5)` stays the checked error.
- **Why:** gates are graph structure like teardown and liveness, so they are read, added and deleted as structure, and nothing gains a third slot. One name per node: several would leave unclear which one a reader means. Names only: an anonymous node has no binding to hold the cache, and the address-keyed index that would replace it was rejected 8 September 2026 (›Meta-navigation‹).
- **Ruled:** 15 September 2026, #123.
- **Open:** after 20 September 2026 it is unclear which default admits a gate write, the default write entry being gone; the 15 September 2026 words "the defaults admit it from the own scope and its descendants" are kept as written.
- **History:** "one ordered set on its record" (7 September 2026) stands as what the binding holds, no longer as where the truth is.
- **Source:** DESIGN.md l.108

### The check is lexical and static: no ambient authority
Access is decided from the viewer's scope chain at elaboration and never looked up by compiled code; the binding stays resident for reflection and re-validation, but running code never resolves a name. Being *called by* a holder grants nothing, an indirect call cannot launder access, and the only way to give a callee access is to hand it the reference. A gate's requirement flows *backward* through the call graph as a static obligation, discharged at the scope that rightly grants it, analyzable from any code section.
- **History:** "erased before codegen" amended 5 September 2026 to "resident, never resolved by running code".
- **Source:** DESIGN.md l.107

### Two access kinds: read and write, on a path
A gate answers one question: *may this viewer read, or write, at this path?* Everything else is one of the two at another path or under another rule: `call` is read of `f.run` without read of `f`'s value (jump allowed, copying the address not, so the reference is usable but not storable or passable); `own` is read of the value plus the *end of the name*, which is the parser's static rule, not a gate; `drop` likewise, with the destructor run; `share` is a read of the pointer, the count being the type's business; a borrow is read or write for a lifetime (the lifetime is the checker's); reflection is read at `x:type`; editing the gate set is write at `x:gate`.
- **Why:** the base rule already said visibility is a read, a borrow a read or write, reflection and graph mutation a node's read and write; the extra kinds were stacked on top, and deleting them makes grants and vetoes statable.
- **Ruled:** 7 September 2026.
- **History:** first an open set: `call`, `own`, and "more (borrow, alias, drop, observe-for-proof) admitted the same way"; superseded 7 September 2026. Text writes `f.code` (slot renamed `run` 17 September 2026, l.189) and `x:dyad` (`:type` since 23 September 2026, l.108).
- **Source:** DESIGN.md l.107

### Effect signatures keep the classification honest; gates run on the stack they analyze
Each operator declares an **effect signature** marking operand slots read or write, and the trusted evaluator stamps the access from it, so evaluator code cannot forge a read into a write. Gates run on the *same* stack they analyze; everything above the first gate-evaluation marker runs ungated, so gate evaluation terminates. That stack is the pass's own (static scope chain and call structure as elaboration sees them), never a runtime call stack; since the one pass builds and rebuilds structure, the chain can change at run time, so a gate is decided when the access is elaborated and re-validated on structural change.
- **Rejected:** a separate meta-stack (a regress).
- **Ruled:** mechanism from design discussion, June 2026, not yet validated; stack clarified 30 August 2026.
- **Seed:** the chokepoint, the lexical tier and the classified `?` seam only; **no prover**. Borrow checker and proof layer bolt onto `?` later.
- **Source:** DESIGN.md l.107

### Gate spelling: words left of `:=`; ownership in the reference type
A gate's spelling belongs to its constructor; there is no global gate syntax. Gates are prefix words left of `:=`: `pub x := 5`, `pub mut x := 5`; `immut x` and `lock x` edit and seal later (›`immut x` removes `mut`, and `lock` seals the gate set‹). Ownership is a word in a reference's type: `a := own @T ?` consumes (callee owns, source empties), `a := @T ?` borrows, `a := share @T ?` co-owns; `-> own @T` hands ownership to the caller. Unmarked is fail-closed: private, immutable, borrow-only. `own` is one word for the kind and the act: `own @T` says the reference owns (a fact about the value, like its type); `own a` reads `a` and ends the name, legal where read is granted and the static rules allow the end.
- **Ruled:** surface settled in discussion, August 2026; `own` as one word, 30 August 2026.
- **Rejected, to stay rejected:** `take` as a separate gate word (30 August 2026).
- **History:** moved right, `x := pub mut 5`, 5 September 2026 (the left of `:=` was a name alone); back left 15 September 2026, since a gate is an entry on the name's binding and the left of `:=` is the name with its gate words (›Declarations are immutable by default‹). Ownership words respelled with the one declaration operator 2 September 2026. "The reference's gate grants `own`" respelled 7 September 2026.
- **Source:** DESIGN.md l.107

### Two grains of access: the name index and the gate
The coarse gate is the name index: a name the trie does not offer a scope cannot be spelled there. The fine gate is an ordinary identity that may read the static call stack and the caller's surrounding scope, but only to *tighten*: denying by call site is fine; permitting on caller identity alone would readmit the confused deputy, barred by "being called by a holder grants nothing".
- **Ruled:** settled in discussion, August 2026.
- **Source:** DESIGN.md l.108

### Sections and the arche; kernel access is a position, not a mode
A **section** is a region of code defined by which names reach it. The **arche** is the root section: the scope a run starts in (command line or REPL), holding every primordial name. Kernel access is a position in the import and call tree: nothing asks "which mode am I in", there is only what a scope can name.
- **Ruled:** settled in discussion, August 2026.
- **Rejected (declined):** mode bits, and the name "user modes": sections divide code; extending them to users later means configuring which names a user's root starts with.
- **Source:** DESIGN.md l.108

### Effect identities are fail-closed; pure computation is ambient
The effect identities are `extern`, the native I/O identities FFI builds (file, network, clock, environment), and the durable store. No section holds them unless a reference was deliberately handed down, so what a dependency can do is read off what it was given. Pure computation, `alloc` included, stays ambient. `extern` is strictest: it is the door out of the graph, and code behind it escapes every in-language gate, so it is granted, never ambient. An effect reference passed into a `pub` function grants **`call` only**: usable, never storable or copyable. A holder that must keep it (a logger keeping its file) needs the caller to open more than `call` explicitly: silence-means-no, extended to delegation.
- **Why:** computing cannot touch the world outside the process; gating it buys noise, not safety.
- **Ruled:** settled in discussion, August 2026.
- **Open (deferred):** resource exhaustion (a section looping or allocating without bound: harm without authority) is real; budgets come later as gates; `alloc` stays ambient meanwhile.
- **Source:** DESIGN.md l.108

### Reflection (`:type`) is fail-closed and gated twice
Reflection is the read at `:type`. Coarsely, by the name index: a section that cannot spell `:` cannot reflect at all (how a language or section is denied it wholesale; why the arche holds it). Finely, by the binding's read entry at each `:type` along a path: an imported section reaching a `pub` name's structure reads the anonymous nodes below it but stops at a private name's `:type`. A binding's other fields (scope, range, gates) are open structure. The August "handed-down view identity" is thus `:` plus the binding gates.
- **Why:** reflection cannot touch the world, but it reads what visibility hides: privacy-sensitive as effects are world-sensitive.
- **Ruled:** settled in discussion, August 2026; `:` as the view 8 September 2026.
- **Open:** is `:` among the ambient names an import resolves? The August default said no view unless handed down; the binding gate makes yes safe for values.
- **Seed:** (August 2026) the view registers ambient like every identity; the fail-closed target waits for the grant path (a capability passed by argument, needing `type`-typed parameters).
- **History:** `:dyad`, retired for `:type` 23 September 2026.
- **Source:** DESIGN.md l.108

### A language is a section with a written start of five names
`language (…)` parses its body in a section holding **five names**, `:=`, `,`, `«`, `»`, `logos`, plus unknown spellings, allowed only left of `:=` (since 10 September 2026 two pattern identities of the start, a word and a symbol run, ranked by `lex_rank`, ›The scope's constructor is the driver‹). Every other name is declared: `if := logos (if)`; what cannot stand alone, `lex «)» := logos (lex «)»)`; a comment, `# := logos (#)`; a word with no meaning yet, `leave := logos (?)`, or `? := logos (?)` once and `leave := ?` after. `logos (…)` is the Logos scope continued (›Substrate vocabulary‹), read with Logos's own bracket, so brackets need not be in the start set.
- **Why:** the five are exactly what the door and a declaration cannot be written without: `logos («)` would open a string; `logos (:=)` and `logos (,)` have no operand and no value to stand as. Nothing the door can fetch belongs in the start. `lex` with nothing to consume stands as its own value, so it is fetched: `lex := logos (lex)`.
- **Ruled:** 3/4 September 2026; `?`, 4 September 2026.
- **History:** six names on the morning of 4 September 2026 (`lex` among them); `lex` left that evening.
- **Source:** DESIGN.md l.108

### A language opens closed; the door back, `logos (…)`, is opt-in
Inside `my_language (…)` only its own names exist. A language admits the door by declaring `logos := logos (logos)`, shaped like `if := logos (if)`; one that does not is sealed, and inside it `logos` is an unknown spelling. `logos (…)` continues **the Logos scope the block stands in lexically**: `english ( she leave at logos (t). )` in a function body reads that function's `t`; at top level `logos (…)` is the plain group. `{ }` is only the `«` and `#` constructors' interpolation spelling, unassigned elsewhere.
- **Why ({ } rejected):** a language block is code with names, not text, and already had a door. Making `{` a bracket every language must honor would reserve a spelling where every spelling belongs to its declaring constructor, and would give a closed section a door its author never declared.
- **Ruled:** 4 September 2026 evening.
- **History:** that morning `{ }` was "the hole to the surroundings" of every closed scope; rolled back that evening.
- **Source:** DESIGN.md l.108

### `type (…)` is the open block
Its body sees its surroundings, which makes `y := 1, g := type (y := 3)` the ruled error. An instance's block follows the language its type was defined in. So a language may give a Logos spelling its own meaning (an English `not`) without collision, since the Logos one is not in its section; and every `my_language (…)` block continues the one scope, the REPL's session model.
- **Source:** DESIGN.md l.108

### Reflectability is a droppable capability; the resident graph is a dial
An interpreted region *is* its LG; a JITed region stays reflectable *through* its LG (›The architecture‹). Keeping that LG resident is the only real cost of the source-to-graph size growth. Reflection is a read under the reader-writer rule, so it can be *revoked*: drop the last read capability into a compiled region, and its LG, with no reader, is reclaimed by the reachability that frees a scope's runtime graph (›Two graphs, one source of truth‹); the region shrinks to its bare callable (a `native`-like leaf, an `@exec` body with no readable source). No collector is added. The cost is set per section: fully reflectable (whole image resident as LG with types and proofs: a Smalltalk/Lisp image, heavier for the proofs); toolchain sealed once booted (LG dropped, callables kept); everything sealed (an ordinary native binary, no graph overhead). The seal is a contract: it keeps what others already read out (types, compiled form, a proof's checked certificate) and gives up re-deriving them (recompile, re-prove, edit structurally, introspect), like stripping symbols from a release build. Only a section's *private* nodes are reclaimed; interned identities and subgraphs another live reader points into stay pinned.
- **Ruled:** direction from discussion, July 2026; not yet validated. "Seal" is a placeholder name.
- **Source:** DESIGN.md l.109

### User-replaceable allocators
`alloc.arena`, `alloc.bump`, `alloc.pool`, `alloc.system` and custom allocators share one interface. Arenas allow cyclic structures through indices instead of borrows.
- **Source:** DESIGN.md l.110

### Two concurrency shapes: `parallel for` and async
`parallel for` does data-parallel work over disjoint indices (the borrow checker recognizes the pattern). Async does I/O-bound work through stackless tasks on user-defined executor pools. A function that can pause is marked `async` and pauses only at an explicit `.await` (allowed only inside `async`), so every pause shows in the source. `async pool (...)` picks the pool for a region. Tasks are anchored once started (no Pin machinery); borrows cannot cross awaits except within the task's anchored state record; capabilities flow through the call graph to enforce this statically. What a task can wait on is an open set, so event listening is user-definable like allocators: one small interface (register interest, promise a wakeup), implementations as replaceable identities. The waker is just "make that task runnable again"; the reactor lives with its pool (io_uring, epoll, a GUI pool on the platform message loop; GPU fences, embedded interrupts, chain confirmations the same way). The stdlib ships standard pools: naming one is the baseline, writing one the escape hatch.
- **Ruled:** settled in discussion, July 2026.
- **Source:** DESIGN.md l.111

### Scheduling, preemption and cancellation happen at boundaries
A task yields at each `.await`. Beyond that, the runtime can step in at any *boundary* where the task's live state is graph data: between interpreter steps, or at the edge of a compiled burst. A hot region may compile to short bursts that read inputs from the task's state, run at full speed, and write results back at each boundary. A paused task *is* its state: no stack maps, nothing rebuilt from registers. Cancelling drops that arena-scoped state like any runtime graph, running its live scopes' pending `defer`s. Only work inside a burst sits in registers, unseen, since control is taken only at boundaries. Burst size is a per-region dial: short bursts preempt and cancel quickly but write state out often and optimize only within a burst; one big burst (a whole hot loop) gives top throughput with few stops, for compute kernels; the longest burst bounds worst-case time to preempt. It rides on the interpret/compile boundary: where an artifact can deoptimize, the scheduler regains control. Preemption is invisible and resumes seamlessly. Cancelling stops a task at its last boundary for good: values dropped, remaining steps never run, so staying consistent across a possible cancel is the program's job.
Two rulings bound the dial:
1. Splitting into bursts is never hidden. Boundaries are placed by the user or an ordinary identity (a backend or pool policy), inserted as reflectable structure, the same law as constructor-inserted teardown; the compiler never adds yield points behind the graph's back.
2. A pool's latency promise (no burst over a millisecond) is not a static WCET proof, which exists only for heavily restricted code. It is empirical: measured history plus the region's structure, enforced by deoptimization. A burst over budget is an observable event that demotes its region to finer bursts or interpretation, so the bound corrects itself instead of being proven.
- **Why:** keeps one task from freezing its pool; makes preemption and cancellation uniform and cheap.
- **Ruled:** the two rulings settled in discussion, July 2026, both in the adaptive layer after v1.0.0. The scheduling model: design discussion, not yet validated.
- **Seed:** the bootstrap can ship cooperative `.await` yielding and add boundary preemption as the compiled tier matures.
- **Source:** DESIGN.md l.112

### Case study: an RCU library's comment-only rules become machine-checked
[TSM](https://github.com/ThobiasKnudsen/LogosMath/blob/gd_to_tsm_and_gtsm/include/tsm.h) is a lock-free, RCU-protected, type-generic hash table in C (userspace-RCU LFHT; every node's type node carries its `fn_free_callback` and `fn_is_valid`: a C forerunner of the dyad). It states every caller safety rule only in doc comments. Each class maps to a Logos mechanism:
- *Conditional ownership transfer* ("take ownership … so you should not free … if successful"): `own`. The caller's place empties at the call, its inserted `defer free` leaves with the value to the callee's binding site (removed from the caller at parse), and a callee that fails hands the value back in its error value, bound by the caller to a new name. Leak and double free become unreachable.
- *"Free only via `call_rcu`"*: the node constructor inserts the RCU-deferred free as the only teardown; RCU-deferred freeing is just another allocator behind the one interface.
- *Forgotten `rcu_read_unlock`*: the read guard's constructor inserts `defer unlock`.
- *Use after grace period* ("a pointer obtained inside the read section must not be used after it"): a lexical lifetime bounded by the guard's scope, ordinary borrow-checker ground. The hardest rule happens to be lexically shaped.
- *"Never `synchronize_rcu`/`rcu_barrier` inside a read section"* (and defer-free must run outside one): effect rules, enforced by capability tracking; those operations need a capability the guard's scope revokes.
- *A type node must outlive the nodes it types*: an ordinary lifetime relation.

The interpreted tier catches the dynamic classes at runtime as checked errors, never UB; the static tiers (borrow checker, capabilities) close aliasing and effect ordering. Together: 100% of the header's stated obligations machine-enforced, where C enforces none.
- **History:** "on failure ownership stays with the caller": superseded 3 September 2026 (›An `own` argument is consumed at the call‹); in v0.1.0 a failed call is a fault.
- **Source:** DESIGN.md l.113

## Mutability and construction

### Writing a value writes in place; the dyad keeps its address
`=` writes the value slot of the existing dyad, so `&x` stays valid after `x = 7`.
- **Why:** identity is address. Rebinding the name to a fresh dyad would break it.
- **Ruled:** mechanics settled in design iteration, June 2026; in-place writing ruled 30 August 2026.
- **Rejected:** rebinding to a fresh dyad on `=` (breaks identity-is-address).
- **History:** June 2026, `mut` was recast as a type modifier and phase bits were proposed as storage. Both are gone, see the History lines of the rules below.
- **Source:** DESIGN.md l.117

### Two operators write a dyad: `:=` declares, `=` reassigns
A dyad is a type slot and a value slot. `:=` introduces a name and binds it to a value, reading the type from the value (`key := value`). `=` writes the value slot of a name that is already declared (`key = value`). Declarations are immutable by default.
- **Ruled:** two operators since 2 September 2026 (`:` as a declaration operator deleted, see below).
- **Source:** DESIGN.md l.119

### Left of `:=` stands one name (or regex), with its gate words in front, and nothing else
`pub x := 5`, `pub mut y := i32 5`, `pub math := type (…)`. The binding is filled where the gate words and `:=` meet on the tape. Nothing on the value side reaches across `:=` to the binding. A gate word anywhere else, `f(mut i32 5)`, is a gate value where none is accepted: the checked error (see ›A gate word stands only where a binding is being filled‹).
- **Why:** a gate is name data, an entry on the binding, never on the value. So it stands on the name's side of `:=`. This also closes for gates the "reach" question *The constructor is a field* had left open, and `pub x := a + b` still gates `x`.
- **Ruled:** 15 September 2026.
- **Open:** a gated return type (it has no name to stand beside). Whether the reference words `own` and `share`, which spell what a reference *does* on binding and stand in its type (*Sections, the arche, and effect identities*), keep that place ("unsure", 15 September 2026).
- **History:** 5 September 2026 the gates were prefix constructors on the right, `x := pub 5`, `y := pub mut i32 5`, binding just above `:=` so `x := pub a + b` gated the sum. Its ground, "a name is a placeholder for an identity, not a thing attributes attach to", was overtaken on 7 September when gates became entries on the name's binding. Replaced 15 September.
- **Source:** DESIGN.md l.119

### A gate word reads the declaration to its right
Each gate is a prefix constructor over the declaration: `pub mut x := 5` is `pub` reading `mut` reading `x := 5`, each filling the binding of the declaration it read. A gate that reads otherwise is that gate's own rule, not a second grammar ("a gate's spelling belongs to its constructor", *Sections, the arche, and effect identities*).
- **Ruled:** 15 September 2026, Thobias: "most gates read the constructor to their right, but it depends on the constructor for the gate".
- **Seed:** the seed's `pub` has done this since August 2026; it never followed the 5 September respelling.
- **Source:** DESIGN.md l.119

### `?` is one value, the unknown; `key := T ?` declares without a value
`?` is the identity its spelling names, one value, never a fresh dyad. A place holds it like any value. `x == ?` is the plain identity comparison. A lookup with nothing to give (`array_mints[t]` on a missing key) hands back `?`. Whether a type's places may hold `?` is that type's own tolerance: a `type` or `dyad` place holds it, a number place refuses it, so `x = ?` or `x == ?` on an `i32` is a plain type mismatch.
`key := T ?` is the valueless declaration: a fresh place per name whose value is `undefined` ("nothing written here yet"). `key := ?` is the bare one. The value comes by a later write. The freshness belongs to the place, never to the unknown: filling `x` writes x's place, so the one `?` is aliased by nothing.
Since `:=` accepts any dyad as its value, a type included, the valueless form needs its `?`: `x := i32` names the type, `x := i32 ?` declares.
- **Why:** Thobias: "i dont want any special cases".
- **Ruled:** 23 September 2026, Thobias.
- **Seed (#137):** a bare `?` stands as the one identity. `?` reads a type to its left only when that type applies to it, not when an operator waits there (`x != ?`). `T ?` still allocates zeroed bytes, which no read reaches.
- **History:** before 23 September, `?` was a constructor building a fresh dyad with both slots `undefined` at every appearance, a literal, so `x := ?` bound x "to its own hole rather than aliasing one global unknown". Replaced because freshness belongs to the place.
- **CONFLICT:** see ›`=` refused on a non-`mut` name vs. filling a valueless declaration‹ at the end of this section.
- **Source:** DESIGN.md l.119

### Reading a place before its first write is refused at parse
The refusal is a gate entry on the binding. From the declaration to the place's first write the entry says `false` to a read; after it, nothing. The run never checks a byte to learn whether a place is filled. The entry belongs to the `?` constructor: the declaration that left the value `undefined` puts it on the binding. It is a distinct entry in the ordered gate set, not a word the author writes.
Which write ends the range: a sibling write in text order fills the name from that line. A write inside a nested block, loop or `fn` body does not, so a read after only such writes is refused.
- **Why:** a "parse time gate is the correct option" (Thobias); no run-time byte, the drop-flag reason of *Explicit heap*. The declaration that leaves a value unwritten is the one that knows it, so nothing is spelled twice and no place can be declared empty and lose the veto. Text position is decided from what the pass has already seen: no path analysis, and fail-closed where a branch might not run.
- **Ruled:** 23 September 2026, Thobias (the same evening, on his own question "maybe it should be a separate gate?").
- **Seed (#137):** the entry refuses at parse every use of the name except the target of `x = …` and the binding read `x:…`, until the sibling write.
- **Source:** DESIGN.md l.119

### A value of several fields is filled one field at a time
`v := T ?` leaves unwritten each field of `T` whose declaration gives no default. `v.f = …` is a write of `f`, never a read of `v`, and a sibling write in text order fills that one field (rule above). Reading a field still unwritten is refused. Any use of `v` as a whole (passed, returned, read) is refused until every field is written. This covers every type with fields and is checked at parse.
- **Why:** the entry exists so nothing reads a place holding nothing. A value of several places is safe to read exactly when each place is. One field's write filling the whole value would let the others be read empty; refusing `v.f = …` left no way to build a value field by field.
- **Ruled:** 26 September 2026, Thobias, choosing "option 1" of three.
- **Rejected:** any field write filling the whole value; keeping `v.f = …` refused (reasons above).
- **Seed:** narrowed 26 September 2026 (Thobias: "filling fields one by one is fine"): for a type whose values are nodes, `v := T ?` is a new empty node and writes to its fields fill it one by one, as a parse fills its node (see *An array expression is a node of `array`*). The general rule widens this to every type with fields.
- **Source:** DESIGN.md l.119

### Each type decides whether it can stand without a value
Tolerance for `?` is authored per constructor (*Deferral is authored*). A type that refuses it cannot stand valueless: fail-closed.
- **Source:** DESIGN.md l.119

### `:=` decides nothing about copy or reference; the type's reading rule does
`:=` binds the name to what its right side yields, and the type defines how the value is read.
- A **data** type reads its value as bytes, stored in a fresh place: `y := x` is a **copy**. Own address, own binding, `y = 7` leaves x at 5, `&x != &y`. y's gates are only what its own left side sets (`pub y := x` a public copy, `mut y := x` a mutable one, `y := x` closed), never x's.
- **`type`** reads a type as the identity itself: its constructor, with nothing to consume, returns the reference, never a copy. So `x := i32` makes `x` another spelling of i32: `x 5` is classified by i32, `x == i32` is true. This is `type`'s own rule, not an exception in `:=`.
- Two names for one data place exist only where written: `y := &x`.
- A value that cannot be copied (an owning `@T`, whose copy would be a second free) is the checked error. Move it with `y := own x` or borrow with `y := &x` (*Memory and concurrency*).
- **Why:** identity is address. A data name yielding its dyad rather than its bytes would make `y = 7` write x and `&x == &y` hold with no `&` in sight: reference semantics without a reference. A copy is the safest default and aliasing already has a spelling. A decision in `:=` would make one operator carry a rule per kind of value, when the reading rule already lives on the type.
- **Ruled:** 5 September 2026 (corrected the same day from a first wording that put the decision in `:=` with types as its exception).
- **Seed:** has always copied data here.
- **Source:** DESIGN.md l.119

### A function reads as a copy
`fn`'s reading rule is the data rule. `g := f` is an independent function record sharing f's body graph; `g.compile()` installs bcode on g alone, f untouched. Only `type` returns the identity.
- **Why:** a fn is a value of type `fn` as an i32 is a value of type i32. A type is what every value's type slot points at, so a copy of a type would classify nothing.
- **Ruled:** 5 September 2026.
- **Open:** `language` and `proof` are left to their own constructors.
- **Source:** DESIGN.md l.119

### A value with its type is written by juxtaposition, type first
`key := i32 32`, anonymous `i32 32`, `f64 2.0`. Never `T = value`.
- **Source:** DESIGN.md l.119

### `:` is not a declaration operator
The declaration forms are `key := value`, `key := T ?` and `key := ?`, and nothing else.
- **Why:** `:` as a type-slot write bypassed the type's constructor, but the type must be constructed anyway, so a second door into the type slot was a second mechanism.
- **Ruled:** 2 September 2026.
- **Rejected, to stay rejected:** `:` as a declaration operator; the composed declaration `key : T = value` (also listed in ›Substrate vocabulary‹).
- **Open:** `:` may come back as pure sugar, `a : T` meaning exactly `a := T ?`, if parameter lists force it.
- **Seed:** retired `:` with the driver convergence (September 2026, #59). It accepts `key := T ?`, `?` being an identity just below application that reads the type to its left.
- **History:** `key : T` and bare `key :` were the old valueless forms; replaced 2 September 2026 by `key := T ?` and `key := ?`.
- **Source:** DESIGN.md l.119

### Names, types and values
A name may carry a type and a value but need not. A type may carry a name and a value but need not. **A value always carries a type** (the type defines how it is read). Names live in the trie; type and value live in the dyad.
- **Source:** DESIGN.md l.119

### `=` on an undeclared name is an error
`key = value` overwrites the value of an already-declared `key`, keeping its type. A mistyped `cont = 6` on an undeclared `cont` is caught instead of silently creating a binding. Declaration is always `:=`; type inference rides `:=`.
- **Ruled:** amended 2 September 2026.
- **Source:** DESIGN.md l.121

### `:=` and `=` bind loosest and associate right to left
They sit at the top of the `parse_rank` order. This lets the language define its own operators: `:= := …` is a well-formed declaration whose left operand is the `:=` spelling itself.
- **Ruled:** settled in discussion, July 2026.
- **Source:** DESIGN.md l.121

### Parentheses are one identity, near the top of the order
`(` is constructed at discovery (*The scope's constructor is the driver*), so a group is one finished cell before any operator around it runs. A call `f(a)` is that same cell consumed by the callee's own constructor, the tight juxtaposition just below it. There is no second bracket.
- **Ruled:** rewritten 2 September 2026.
- **History:** July 2026 wording: grouping paren loosest, call paren tightest. It described the superseded model; replaced 2 September 2026.
- **Source:** DESIGN.md l.121

### A constructor that takes a type as operand writes its own `parse_rank`
`array i32 (1, 2, 3)`, `fn () -> i32 ( body )`: the constructor's `parse_rank` sits above the juxtaposition level, so `array` takes `i32` before `i32` could take the group as a conversion. The number is written in the identity's own body, as `fn`, `if` and `for` write theirs. `type` supplies no default for a type's own parse that takes a type.
- **Ruled:** in discussion, 2 September 2026.
- **Source:** DESIGN.md l.121

### A parse is ranked by what it takes from its right
- A parse that takes a bracket to its right (the type `array i32` taking `(1, 2, 3)`, an array value taking its `[k]`) sits at application's rank, spelled `dyad.parse_rank` (the rank `dyad (i32, 7)` has).
- A parse that takes a type which may itself have a bracket after it (the chooser `array`, as `fn` takes its `-> i32`) sits at `fn.parse_rank`: above application, below `.` and `:`.
- A type body that fills `share parse` and leaves `share parse_rank` unfilled gets application's rank, as `fn` gives its values the call defaults. (The 2 September "no default" is about a type's own parse that takes a type.)
- `*.parse_rank + 1` (array.logos took it from power.logos on 23 September as a stand-in) stays right for `^`, an infix that reads both sides once built, and is wrong for these three: that far below `(`, `i32` has already taken `(1, 2, 3)` as a conversion before the chooser runs.
- **Why:** `(…)` or `[k]` right after a value is the same act as a call, so it takes the call's place; what holds for `f(2).x` then holds for `a[1].x` and `array i32 (1, 2).size`. Ranks spelled relative follow `dyad` and `fn` if those move, and no new number is invented.
- **Ruled:** 25 September 2026. Thobias delegated the choice to Claude ("do what you think is best"). Closes the #137 open question on the ranks of array.logos's three parses.
- **Seed:** the numbers are already the seed's (application and an unfilled `share parse_rank` 91, `fn` 92). Two blockers were recorded open and closed 25 September 2026: a parse written in Logos running at discovery could not read past what is lexed (the #61 stand-in, closed by the lazy-read ruling of *The scope's constructor is the driver*), and the chooser reading `i32` would wake `i32`'s own read of the bracket (closed by *A cell a constructor reads from the tape arrives unbuilt*: the chooser gets `i32` unbuilt and `(1, 2, 3)` as a bracket; "just below it" means a bracket is built as it is lexed and a juxtaposition is not). As written on 25 September, array.logos still stopped at `tape[0] = get_mint(t)`, a cell read from the tape not being taken as a `type` argument.
- **History:** as ruled it said "a fields block that fills `shared parse`"; the fields block is gone (25 September 2026, *A type body describes one level*) and `shared` is spelled `share` (26 September 2026).
- **Source:** DESIGN.md l.121

### `mut` is a gate on the binding, not a type
Every declared name's binding holds its gates. `mut` is the write gate on the value slot, a prefix word on the name like `pub`. `pub` widens reading to every scope; `pub mut` widens both. A name without `mut` is written nowhere once filled. `mut` is not scoped. Spellings: `mut x := 5`, `pub mut x := 5`, `mut x := i32 ?`, a field's `mut x := f32 ?`, a parameter's `fn (mut a := i32 ?)`.
- **Why:** a value's permissions are facts about identities, not about machine types, so one binding lookup answers reachable, live and permitted. The seed's liveness range already had that shape (graph truth on the declaration, the binding as the fast path). `mut` in the type made every type carry a second identity and made inference copy mutability from value to name.
- **Ruled:** 5 September 2026. Scope clause ruled 7 September, removed 20 September 2026 (`mut` is not scoped, *Read and write are one mechanism*). Gate words left of `:=` since 15 September 2026.
- **Rejected, to stay rejected:** `mut T` as a type; **name-mutability**, `mut key` as a renameable trie entry: nothing used it, and it made the commonest declaration mean something exotic. Deleted 5 September 2026.
- **History:** first recorded as "`mut T` is the type of a mutable T" (June 2026): mutability stated by the type, `&mut T` the same `mut`, per-field mutability on each field's type (`f := mut F ?`), and `mut` left-prefixing any level of the classifier tower, giving four construction states (`i32 32`, `mut i32 32`, `mut type i32 32` reinterpret-only, `mut type mut i32 32`). Also `mut key` for name-mutability and `mut (…code…)` for a structurally mutable region; these stratified as compilable value writes, deopting type writes, interpreted `mut (…)` regions. A 4 September conflict with `pub mut x := 5` was resolved 5 September by this rule. Before 15 September the gate was spelled `x := pub mut 5`.
- **Source:** DESIGN.md l.123

### A writable pointer is made by `mut` on what makes the pointer
`mut alloc 1 of i32 ?` and `mut &a` give a pointer one writes through. `alloc` and `&a` give a pointer one only reads through. `mut &a` is legal only on a `mut` place. The permission travels in the pointer: a write through `p` is judged from `p:type`.
- **Why:** Thobias: "to get mutable pointer you need to write mut alloc". Only a pointer from `mut alloc` may be written through.
- **Ruled:** 20 September 2026, Thobias. 25 September 2026, Thobias: identities/array.logos allocates with `mut alloc`, so `fill`'s element writes and `a[k] = v` are licensed.
- **Open:** how a parameter spells the writable pointer's type, and how that sits with ›A type never carries mutability‹.
- **Seed:** `mut alloc` accepted since 25 September 2026; the pointer carries no mark yet, and a write through `@` is unchecked.
- **History:** the older spelling `&mut` (`&`'s own spelling) was respelled 20 September 2026.
- **Source:** DESIGN.md l.123

### One binding per declared name
The trie entry itself, named **`binding`**, holds the name's scope, its liveness range, its gate set and the pointer to its dyad. `:` reads it (*The dyad's read surface*). So `pub y := x` makes y a public copy of x's value and leaves x private and untouched.
- **Why:** the alias decides it. `x := i32` binds a second name to i32's own dyad; with a shared identity binding, `pub x := i32` would make i32 itself public and `lock x` would seal i32. Per-name bindings keep `x:gate` x's and `i32:gate` i32's while both point at one cell. No other case parts a name from its value (`:=` copies, `y := own x` gives y a fresh place), so the split answered nothing and cost a pointer chase on every lookup.
- **Ruled:** 7 September 2026.
- **History:** 5 September 2026 split a name entry (scope, liveness range) from a separate identity binding (lifetime, gates); replaced 7 September. The entry was once called `id_context`.
- **Source:** DESIGN.md l.123

### A gate word stands only where a binding is being filled
Left of the name in `:=`; in a field or parameter declaration (the same operator); and a return type (open, see above). On a constructed node a gate word is the checked error: `f(mut i32 5)` is written as the gate on the parameter, `fn (mut a := i32 ?)`.
- **Why:** a constructed node has no binding (›Substrate vocabulary‹). What guards it is that almost nothing knows where it is. A gate needs a binding, and a binding per calculation step is a cost the substrate refuses.
- **Ruled:** 7 September 2026, restated 20 September 2026.
- **History:** first wording, "an anonymous value is gated the same way because it has the record and never needed the name", replaced 7 September 2026.
- **Source:** DESIGN.md l.123, l.129

### A type never carries mutability
`array i32` is one interned type whether or not its elements are written. `+` over a mutable i32 resolves to `add_i32` with nothing to strip. Rewrite patterns never see a `mut`. `y := x` does not copy x's mutability: the type is read from the value, the gate defaults to closed, and `mut y := x` says otherwise.
- **Ruled:** 5 September 2026.
- **Source:** DESIGN.md l.123

### An access along a path needs every binding on the path to grant it
`b.x = 5` needs the write gate on `b` and on the field declaration `x`. So an immutable binding protects its contents, and a frozen field stays frozen inside a mutable one. Gates compose under the Kleene rule of *Read and write are one mechanism*.
A **dereference starts a new path**: `b.r@ = 5` writes at the address `r` holds, not inside `b`, so it needs only the reference's own write-through permission. A reference binding therefore carries two gates: write to its slot, and write through it (the permission *Reference-granular permissions* puts on the reference).
- **Ruled:** 5 September 2026. Through the type: 20 September 2026, Thobias: `t.y = 4` needs `mut` on the `share` member alone, the type standing as a namespace there, not as a container.
- **Seed:** since 20 September 2026, `q.v = 3` needs `mut` on `q` and on `v`; `t.y = 4` needs it on the member alone. A type's own parse fills a field through the field's default entry (›Fields start writable by the type's own parse‹).
- **Source:** DESIGN.md l.123

### A field's gate lives once per type, never per instance
A field is bytes at an offset with no binding of its own (the same fact that keeps `own` to bare names). Per-instance restriction is done at the instance's binding: `a := p ?` against `mut a := p ?`.
- **Ruled:** 5 September 2026.
- **Source:** DESIGN.md l.123

### Bindings are checked at elaboration, never by running code
The binding is consulted at every elaboration: parse, graph mutation, reflection. It is not erased; bindings stay resident for the whole run. Compiled code simply never resolves a name.
- **Ruled:** 5 September 2026.
- **Source:** DESIGN.md l.123

### Constructed nodes have no gates; declared names get a default read entry
An anonymous identity (a constructed node, no binding) has no gate, so it has full read and write permission everywhere: the rewriting engine, the optimizer and every constructor work on exactly those nodes. A declared name gets the default read entry, and no default write entry: without `mut` it is written nowhere.
Permission is the gate's question; discipline is the borrow rule's. A write to a constructed node is still a structural write: it needs exclusive access and drops every compiled reader first (the deopt of *Compilation is a reader of frozen structure*).
- **Ruled:** 7 September 2026; default write entry removed 20 September 2026 (*Read and write are one mechanism*).
- **Rejected:** a `gates` field inside `type` (or its instance part) so that anonymous nodes built by advanced algorithms get gates of their own. An algorithm gates what it publishes by naming it: it writes the declaration as text that is lexed and constructed, so the name's binding carries the gates, and the path rule of *Meta-navigation* covers what hangs from it at no storage. Why: gates guard names, naming a node is how it is handed to the rest of the program, and the check stays at parse. Declined 20 September 2026, Thobias.
- **Source:** DESIGN.md l.129

### Fields start writable by the type's own parse
A field declaration starts with a write entry for the type's own `parse` (so the parse can fill the node it builds, now spelled `tape[0].f = …`). `immut f := i32 ?` removes it: a veto on the write wherever the name is reached, so `immut mut x := …` is written nowhere (Kleene rule). `mut x := f32 ?` adds the write from anywhere the instance is reached, `b.x = 5`.
- **Why:** Thobias: "to make gates non ambiguous there should then be a default gate where the fields are writable during parse time and it can be overriden if you want it immutable in parse as well". One mechanism: every permission an entry, none an exception.
- **Ruled:** 20 September 2026, Thobias. The word: "immut is the word" (same day).
- **Seed:** both since 20 September 2026.
- **History:** ruled over the fill `this.lhs = tape[-1]`; `this` was removed 26 September 2026 and a parse writes fields as `tape[0].f`.
- **Source:** DESIGN.md l.129

### What still holds of the old lifecycle
`undefined` is a null slot, and reading it is a checked error, never undefined behavior. Whether a cell is constructed during parse is the tape's `is_constructed` list, never the dyad's. A type write (reclassifying a live dyad) must discharge **validity** (the existing bytes are a valid value of the new type) and **layout** (the new type reads the same shape, or storage is reallocated); the refinement/proof layer discharges these where it can, `unsafe` where it cannot, like a checked `transmute`. A structural graph edit that rewrites a dyad's type triggers re-validation, as every `graph_mut` write does.
- **Ruled:** 7 September 2026 (what stands of the June lifecycle).
- **History:**
  - Three-state lifecycle `undefined` → defined → frozen for both slots, with `mut` on the classifier keeping a slot writable past definition, and exclusivity making undefined-slot checks local ("the flip *is* publication"). Superseded 7 September 2026: there is no phase and no frozen state; writability is the gate on the binding.
  - Phase stored as tag bits in the low four bits of the 16-byte-aligned type pointer, not as wrapper nodes (a wrapper breaks type transparency); reflection could synthesize a wrapper view. Proposed June 2026, superseded 7 September 2026: `undefined` is a null slot and writability is a gate.
  - "The default gate of a declared name is write allowed while the slot is null" (7 September 2026). Removed 20 September 2026.
- **Source:** DESIGN.md l.125, l.129

### `immut x` removes `mut`; `lock` seals the gate set
There is no phase to flip. `immut x` removes the `mut` entry from x's gate set. "Frozen" names that resulting state; it is not a gate word. Editing a gate set is a structural write: exclusive under the borrow rule and re-validated like every `graph_mut`. So a name may go `mut`, then `immut`, then `mut` again.
**`lock`** is an entry that denies every later change to the set it stands in, itself included, so it is one-way by construction: `x := lock mut 5`, or `lock x` later. After it neither `immut x` nor a new `mut` can pass.
- **Why:** a reader that draws a conclusion from a stable dyad keeps the conclusion after dropping its reference. Once gates are editable, that guarantee must be opted into. `lock` is the opt-in, which a compiled reader, a type's layout at first instantiation, or a proof may demand on what it reads, instead of every dyad paying for a permanence most never need.
- **Ruled:** in discussion, 7 September 2026.
- **Rejected, to stay rejected:** `metamut`, a separate grant of flip authority (delegating flip authority is just ownership transfer).
- **CONFLICT:** l.127: "a name may go `mut`, then `immut`, then `mut` again" vs l.135 (›Substrate vocabulary‹): "Recorded as rejected, to stay rejected: `metamut`, the reverse flip, …". The reverse flip was rejected because "facts derived from reading a frozen dyad outlive any reference to it … un-freezing is unsound without universal dependency tracking" (l.127, superseded record).
- **CONFLICT:** l.127: "after which the default gate (write allowed while the slot is null) refuses every further write" vs l.107 / l.129: "the default write entry gone 20 September 2026 … a name without `mut` is written nowhere".
- **History:** the one-way phase flip `immut x` (June 2026): owner-only with no live borrows, like a move; frozen was final so a flip never invalidated anyone. Respelled 7 September 2026 into gates plus `lock`.
- **Source:** DESIGN.md l.127

### `share` places a member once; unmarked members are per value
In a type body, `share x := …` is one place stored with the type, reached through every node (`p.dims`) and through the type itself (`point.dims`). An unmarked member is a place per node whatever its gates: `size := u64 0` is a per-node default, `share mut mints := …` is one mutable place every use of the type writes.
`share` is a gate: an entry on the name's binding beside `pub` and `mut`, read where they are read (`x:gate`). It decides placement where the others decide access. It stands first: `share mut x := …`.
The partition stands: layout is the unmarked members (the per-instance layout is derived, like C++'s struct), namespace is the `share` members (stored once, like C++'s namespace). A type whose members are all shared is a pure namespace, which is what a language is; the merge of `type` and `struct` into the one identity `type` is this partition read to its end (›Substrate vocabulary‹).
- **Why (placement by a word):** deriving placement from mutability left shared-and-mutable state unspeakable, so a generic's memo needed a sibling identity (18 September); a reader had to apply a rule to know where a line lived instead of reading a word on it; and the derivation already had an exception, a per-instance constructor being immutable yet per instance (*The constructor is a field*).
- **Why (a gate):** Thobias: "shared is a gate, put it on the record". One family of words left of `:=`, one home, one reader.
- **Ruled:** 19 September 2026, Thobias (placement by the word); 20 September 2026, later, Thobias (it is a gate). Partition: July 2026, respelled 4 September 2026.
- **Seed:** since 20 September 2026 the reader puts it on the member's binding, first in the set. Since 24 September 2026 (#141) a node's `.` finds a `share` member where no field of its own answers.
- **History:**
  - Derivation (July 2026): a field ever mutable, even only during `init`, is per instance; one never mutable is stored once with the type; placement decidable at the declaration site. Superseded 19 September 2026 by the `share` word (reasons above).
  - 19 September 2026: `share` was "placement, not a gate". Superseded 20 September 2026.
  - Members stood in a `fields = (…)` block (spelled `instance` until 19 September evening), and the type read them as `point.fields.dims` (23 September 2026: "point.dims is actually not allowed"). The block was removed 25 September 2026 (*A type body describes one level*): members stand in the type body, and `point.dims` reads through the type.
  - The mark was spelled `shared`; renamed `share` 26 September 2026, Thobias ("one letter less").
- **Source:** DESIGN.md l.131

### A type's layout locks at its first instance
A type's layout-relevant slots must be defined before its first instantiation. The first instance adds `lock` to those slots' gate sets.
- **Why:** a layout read by an instance is a conclusion that outlives the reader, exactly the case `lock` exists for.
- **Ruled:** stated as consequence of the 7 September 2026 `lock` ruling.
- **Source:** DESIGN.md l.131

### `share` means one place, the same value at the same time, everywhere it is written
Its value is made once, at the definition. Where the line stands only decides which contexts reach it:
- in a type body: every node of the type and the type itself;
- in a function body: every call (C's `static` local). `share mut n := i32 0, n = n + 1` counts the calls. A `share` line in a loop or branch inside the body is still the function's. Two functions each own their own place under the same name;
- at a file's top level: every importer of the file in the run (a file loads once per run already; `share` makes it a guarantee that holds even if loading changes);
- in a loop or branch outside any function: every pass (an unmarked name is made again on each pass).
"At the same time": one value for every task and thread, never a per-task copy (async and threads come after v1; recorded so they inherit it).
Without `mut` the name is not rewritten, while a map behind it is written through it as through any place. The initializer runs **once, when the definition is parsed**, not at the first call. So it cannot read the function's parameters or locals, or a name of the loop or branch around it (no value there yet): the checked error. A `print` in it prints at the definition.
- **Why:** Thobias: "it just means that this value is the same at the same time across all contexts". One word, one meaning: a reader never has to ask where a `share` line stands to know what it does. A call frame is an instance of its function (›Resolution is one rule‹), so a `share` name in a body is what `share` already is in a type. A cache a function keeps for itself (a generic's mints, identities/array.logos) lives inside the function, not beside it as a top-level name. On files, Thobias: "maybe you want the same shared value across all imports?"
- **Ruled:** 25 September 2026, Thobias (function bodies: "it can be used on names inside functions to get some persistent storage across runs of the same function"); later that day, Thobias ("shared should be available everywhere"). Initializer at definition: Claude's choice, agreed by Thobias the same day ("agree").
- **Open (Claude's choice, open to Thobias):** `share` stays refused on a parameter, whose value each call supplies (*A function's surface*), and on a type body's own line, which fills a slot (*The constructor is a field*).
- **CONFLICT:** l.131: "`shared` stays refused … on a type body's own line, which fills a slot …, a type's shared member being declared inside `fields = (…)`" vs l.207: "every line stored once per type says so, `share parse_rank = …`, `share associativity = left`, `share parse = (…)` … an unmarked slot fill is the checked error".
- **Seed:** since 25 September 2026 every position gets a global place, the initializer runs once as the definition parses, an owning value there is freed at program end, and the line itself then only names the place. A function whose `share` name holds a map is not compiled (the checked "cannot be compiled yet" every map in a compiled body gets).
- **History:** 19 September 2026, `share` was a reserved word only inside a type's field block ("yes reserved inside instance inside type"), an ordinary name elsewhere. Widened 25 September 2026 to function bodies, then to everywhere.
- **Source:** DESIGN.md l.131

### Resolution is one rule
A type keeps the defining scope it was constructed from. `a.x` asks `a`'s type for `x`, and climbs no further inside one `.`. The declaration found decides what is found: an unmarked member is a place (the byte offset inside `a`'s value area); a `share` member is the stored dyad. A node reads `p.dims`; a type reads its own `share` members through itself, `point.dims`. Visibility is one extra test at the same lookup. A call frame is an instance of its function: parameter declarations resolve to a frame exactly as field declarations resolve to an instance. Lookup continues up the type chain and ends at the `type : type` self-loop.
- **Ruled:** precision 30 August 2026; the mark (not `mut`) decides what is found, 19 September 2026; reads through the type, 25 September 2026 (*A type body describes one level*).
- **Seed:** since 24 September 2026 (#141): a node's `.` finds a `share` member where no field of its own answers; `t.fields.x` reads one through the type and `t.x` is a checked error pointing at `t.fields.x`. (That was the 23 September spelling; the 25 September ruling makes `t.x` the read.)
- **History:**
  - 30 August 2026: `a.x` probed the type's field scope, then `a`'s own bare scope; mut decided place vs stored dyad. 19 September: the mark decides.
  - 23 September 2026, Thobias: `point.dims` refused, `point.fields.dims` the read. Reason then: one word, `fields`, as the step from a type to what its instances hold. Superseded 25 September 2026 when the fields block was removed.
- **Source:** DESIGN.md l.133

### CONFLICT: `=` refused on a non-`mut` name vs. filling a valueless declaration
l.119 (live, 23 September 2026): "`key := T ?` is the valueless declaration and `key := ?` the bare one, the value taken by later reassignment (`key := T ?`, then `key = value`)", and (26 September) "`v := T ?` leaves unwritten each field … `v.f = …` is a write of `f`", with no `mut` in either example. l.107 (20 September 2026): "a name without `mut` is written nowhere, so `x := i32 ?` without `mut` can never be filled, 'it should not be writable at all'". l.123: "An access along a path is permitted only when every binding along the path grants it: `b.x = 5` needs the write gate on `b` and on the field declaration `x`".

**Substrate vocabulary** (DESIGN.md l.135, a passage inside ›Mutability and construction‹)

### The cell is the `dyad`, its slots `.type` and `.value`
- **Why:** newcomer-plain words win.
- **Ruled:** August 2026.
- **Rejected:** the July Greek triple (cell `synolon`, slots `logos` and `hyle`); `synolon` and `hyle` retired everywhere.
- **Source:** DESIGN.md l.135

### The ground is one identity, spelled `type`; `logos` is a language
Every classification chain ends at the `type : type` self-loop: `3 : i32 : type : type`. `type` is the definition keyword (`x := type (…)`), `type ?` is a place for a type, `-> type` a function returning one, `x:type == type` the ground test. What `type` defines is one thing with two defaults: a type with fields derives its layout; a bare definition leaves parsing and semantics to its author. So `struct` stays merged into `type`. The alias door `x := i32` stays open.
`language` is an identity of type `type`, not a second ground. Its constructor opens a body as a **closed section** (*Sections, the arche, and effect identities*). The Logos language is `logos : language : type`; the identity `logos`, the `logos` binary and the `.logos` extension share one spelling because the language is what all three name. Inside Logos code, `logos (…)` continues the Logos language's own scope, so it is the plain group. From inside another language it is the door back: `if := logos (if)` binds Logos's `if`, which stands as its own value under the decline rule (*The constructor is a field*). In a language's definition body the door is one of the five starting names; in its blocks it exists only where the definition admitted it, `logos := logos (logos)`.
- **Why:** a type-rooted chain is what a newcomer expects; a language being one identity among the types it defines lets a new language be written as one. For `type` over `word` (19 September): the design turns on two levels, and `type` names the first where `word` names a spelling. (As ruled, the two levels were "a type's own lines and its fields block"; the fields block is gone since 25 September 2026.)
- **Ruled:** 4 September 2026 (substance); `type` again 19 September 2026, Thobias.
- **Rejected, to stay rejected:** a separate `struct` core identity (merged into the ground; `language` is distinct from it by the 4 September ruling).
- **Seed:** spells the root `type` and keeps `logos` as a transitional alias of it until the `language` identity exists to give `logos (…)` its ruled meaning. #131 closed unneeded; the README spells `type`.
- **History:**
  - July 2026: `logos` was the ground identity's proper name and the definition keyword, and "`type` and `struct` … never as separate core identities". Superseded 4 September 2026; sketches and examples respelled.
  - 18 September 2026, Thobias: the ground spelled `word` ("everything we write are words"; a definition gives a word its spelling, parse, run and what its nodes hold, more than a traditional type states). Objections recorded then as outweighed: the binding is the word level (in `^ := word (…)` the word is `^`), types with no word (`fn (…)`, `@i32`, `array i32`, an anonymous definition), data types that are not words (`point`), and DESIGN's prose using "word" for a spelling. Reverted 19 September 2026: the two-level split confirmed those objections. The dyad field stayed `.type` throughout ("dyad.type doesnt need to be changed to dyad.word").
  - Ground test was spelled `x:dyad.type == type`; `a:type` replaced `a:dyad.type` 23 September 2026.
- **Source:** DESIGN.md l.135

### `undefined`, `?`, `@void` and `native`
- `undefined` is the hole: nothing written here yet. It carries a definedness obligation; reading it is a checked error.
- `?` is the **unknown**: a deliberate, classified hole the author writes in place of code not yet designed. It is inert until filled and makes any build that reaches it incomplete by construction. It says "the value is not yet known": a standing synthesis goal that a maturing Logos discharges by the proof layer's backward proof search and coherentist plausibility ranking, moving a `?` from left-alone to suggested to filled-and-verified. (See also ›`?` is one value, the unknown‹.)
- `@void` is a type-erased address whose interpretation comes from elsewhere: the honest type of the dyad's value field, since interpretation always comes from the type pointer.
- `native` is a callable-only function: a machine-code body (`@exec`) with no Logos source, opaque to reflection (invoked, never read into). It differs from `undefined` and `?` because it runs and reaching it is no error.
- **Rejected, to stay rejected:** `any` (removed and split into `undefined` and `?`).
- **Seed:** all primitives are `native`, ported to reflectable Logos source one identity at a time (*The architecture*).
- **Source:** DESIGN.md l.135

### Rejected, to stay rejected (vocabulary and structure)
- `metamut`.
- The reverse flip (see CONFLICT under ›`immut x` removes `mut`; `lock` seals the gate set‹).
- `any`.
- A separate `struct` core identity.
- The plural "logoi" in any Logos writing: `logos` is invariant; a dyad pluralizes as dyads.
- A per-record field-name hashtable: field names live in the shared name index, scope-filtered (one resolution mechanism).
- Inheritance.
- Imitating `while` with a stepped `for`: a per-iteration condition deserves its own spelling.
- The composed declaration `key : T = value`.
- `:` as a declaration operator (2 September 2026).
- `mut T` as a type, and name-mutability (`mut key` as a renameable trie entry) (5 September 2026).
- **Source:** DESIGN.md l.135

### Three words for what a dyad is called
- A **spelling** is a trie key: `a`, `+`, `(`, `?`, and `1` through the number regex. Every spelling resolves to a **declared identity**: a dyad with a binding.
- A **constructed node** is a dyad a constructor builds while parsing or running. In `a = a + 1` there are three: the application of `=`, the application of `+`, and the value the number regex builds for `1`. Each has a type that is a declared identity, so it is reachable by its type's name and by structure from its parent; it has no binding of its own.
- "Anonymous" means constructed node: the calculation steps. A literal's value is one even though its spelling is not. A constructed node takes no gate word, having no binding for one.
- **Ruled:** 7 September 2026.
- **Source:** DESIGN.md l.135

## Native interop and the Rust bridge

### Logos calls native code as black-box C-ABI calls; it does not host other languages
The runtime is a Rust program built by rustc (LLVM); Cranelift JIT-compiles Logos code at run time. A foreign function is called in-process, by address, through the platform's native calling convention (the one `extern "C"` selects, and the one Cranelift JITs like wasmtime reach through trampolines). So even JIT-compiled Logos calls native code with no linker step, whichever backend compiled either side. The boundary is the usual one: `extern "C"` signatures, `#[repr(C)]` data, panics caught before they cross, an `unsafe` contract the borrow checker cannot see past. Richer Rust types (`Vec`, `String`, generics) go through a thin per-function shim a binding macro can generate.
- **Source:** DESIGN.md l.139

### The Rust toolchain is needed to build Logos, not to run it
Logos ships as a standalone interpreter. Native code comes in four ways: compiled in as core builtins; loaded at run time as separate `cdylib` artifacts (interpreter and toolchain ship apart); optionally built on demand by invoking `rustc`, then loaded; or isolated behind IPC where a crash must not spread.
- **Why:** the bridge is the cheap path to reusing existing ecosystems.
- **Rejected:** not rejected but kept apart: hosting a language's full semantics as a Logic Graph dialect, a separate, heavier option.
- **Source:** DESIGN.md l.141

## Error handling

### Target: errors are values (`T!`), handled by `match`, passed on by `try`
Errors are tagged unions `(T | Error)`, spelled `T!`: `fn () -> i32!` hands a recoverable error to its caller. A fallible function declares its error types. Callers handle results with `match`, with `success` / `fails` combinators for yes/no outcomes. No exceptions, no hidden propagation: `try f()` inside a `!` function hands the callee's error to the enclosing function's caller, sugar over `match` (Zig's shape). `T!` is for declared fallibility only.
- **Why:** one visible word per call site.
- **Ruled:** `T!` 30 August 2026; `try` 31 August 2026, in discussion.
- **Seed:** none of it in v0.1.0 (see next rules). It lands later as library and driver work over tagged unions; these shapes stay the target.
- **Source:** DESIGN.md l.145

### A checked error is a fault: the task that hit it is cancelled
A checked error is never part of a declared type, so it is not a value but a **fault**. The task is cancelled as ›Scheduling, preemption, and cancellation‹ cancels a task: stopped at its last boundary, pending `defer`s of its live scopes run, no further steps; the diagnostic is the reason. In a single-task program: abort with a message, teardown done. The refinement tier turns faults into compile-time obligations where opted in. A constructor meets a fault only on a broken internal invariant (the "cannot happen" class).
- **Ruled:** 30 August 2026.
- **Source:** DESIGN.md l.145

### Target: a constructor returns `void!`, so a user's syntax error is a recoverable value
The constructor builds the error with `error «…»`. The driver (the enclosing scope's constructor) records the diagnostic, marks the cell, and resynchronizes at the next segment boundary or closer.
- **Why:** one parse yields many diagnostics, which tooling needs.
- **Ruled:** 31 August 2026, in discussion.
- **Seed:** v0.1.0 constructors are `-> void`; the resynchronizing driver comes later.
- **History:** 30 August 2026: constructors "carry no `!`"; replaced 31 August.
- **CONFLICT:** l.145: "constructor signatures in v0.1.0 are therefore `-> void`"; l.189 (17 Sept): "the `fn (tape := parsing_tape ?) -> void` wrapper a parse slot carried is gone ... a slot body has no parameter list at all". Whether a `parse` body has a signature (and a `void!` target) is unclear.
- **Source:** DESIGN.md l.145

### v0.1.0 knows faults only; `error «…»` aborts the run
No `T!`, no `try`, no `match` over errors. `error «…»` is the one primitive: it never returns, the run aborts with its message, one diagnostic per run.
- **Ruled:** 2 September 2026, in discussion.
- **Source:** DESIGN.md l.145

### `error «…»` reads `{…}` exactly as `print «…»` does
Each `{…}` is run and shown as the echo shows it; `\{` and `\}` are braces as text.
- **Why:** one quote reader for both output words, so a message reads the same printed or raised. Thobias: "print and error should use { the same way".
- **Ruled:** 25 September 2026, Thobias.
- **Source:** DESIGN.md l.145

## Meta-reflection and the unified system

### The Logic Graph contains everything, the compiler and itself included
- **Parsing rules:** every token, operator, `parse_rank` and associativity is a queryable node. The parser is not context-free because it consults the trie, which is part of the graph. Tools become language-aware by looking into the graph, not by reimplementing the syntax.
- **The compiler:** above the bootstrap seed, borrow checker, type checker, rewriting engine, optimization passes and lowerings (Logic Graph → Logos IR → backend) are Logos programs and graph subtrees. The seed is the frozen exception: reflectable, not rewritten. An optimization is library work; a platform is the backend interface plus rules.
- **The standard library:** not special; ships with the system, readable and (carefully) overridable.
- **Docs, positions, comments, metadata:** docs and comments are one thing, `#`-built prose nodes, void-valued (›Text literals are plain values‹). Cross-references are edges; positions are node properties held in the derived source map.
- **Proofs and specifications:** a refinement is a subtree of a signature, a proof a subtree of a definition; refactoring either uses the same tools.
- **Later, the IDE:** editor, highlighting, refactoring, debugging UI, configuration as hot-reloadable Logos over the graph, structural editing as library functions (Smalltalk's vision in a systems language).
- **Later, the running application and the browser:** since the IDE is Logos over the graph and Logos has I/O and portable backends, the same structure grows without a rewrite into a networked, rendering platform, a browser reached from the tooling end. See ›The IDE as platform, and the reflective web‹.
- **Source:** DESIGN.md l.149-157

### Paradoxes are handled by stratification, only if needed
If type strata reason about themselves or proofs quantify over the proof system, a universe hierarchy (as in Lean 4) keeps things consistent; only explicit meta-mathematics meets it. With proofs as rewrite rules it is not needed for the ruled shape; it returns only if rules over rules are shown to admit a paradox (›The proof layer‹).
- **History:** qualified August 2026, once proofs became rewrite rules.
- **Source:** DESIGN.md l.158

## Representation, elaboration, and lifetimes

How the Logic Graph is represented, how source becomes runnable, and how structure made at run time relates to source.

### A dyad is two pointers, `type` and `value`; its identity is its address
Sixteen bytes. Any allocator can hold it (no arena needed; the address is the id). Every dyad is an identity; most have no string name and are found by pattern-matching over the trie and graph. With a handle a dyad is directly addressable, but the graph is not addressable by position or name without context: source means nothing without its context, so lookup from text walks the trie (the parser's trie), plus indexes where repeated reflection needs them.
- **Source:** DESIGN.md l.164

### A dyad's type defines how its value is read, and everything else
The type's definition, itself a Logic Graph, gives layout, how the dyad takes surrounding tokens while parsing, when it lowers to Logos IR, and the IR for each well-checked case. A `+` over mismatched or sizeless types does not lower until resolved.
- **Why:** MLIR's idea (a node whose definition gives its parsing, operands, lowering) on one uniform two-pointer cell, joined with the reflective parser.
- **Rejected:** MLIR as the IR. It is only a candidate backend. The Logos IR must be a Logic Graph layer so the same rewriting engine, type strata and proofs apply; an opaque outside IR cannot give that.
- **Source:** DESIGN.md l.164

### Operands are reached through `value`, never stored inline
A binary `+` is one 16-byte cell pointing at its operands. An operand that uses a name is the name's binding; a constructed operand is the node itself (›The dyad's read surface‹, 8 Sept). `value` points at an operand binding whose layout the definition gives: for a binary operator, (LHS, RHS) plus a third field once the operand type resolves, `[LHS, RHS, type]`, so run, compile and reflection dispatch on the stored concrete operation. That field is filled by a lowering rule, never by the operator's constructor at parse (›The callable ground is `@exec`‹). A call's value is its argument binding (one field per argument; one-field anonymous binding for one argument); a variadic call's is sized per call site. A list is never inline: a sequence's expression array sits behind one pointer, an `array` value the scope dyad's value points at. Which machine `+` runs comes from operator plus operand type.
- **Ruled:** lowering-rule fill 8 September 2026.
- **Source:** DESIGN.md l.164

### A number literal is `rational_number` until context gives it a type
It can become any `int`, `uint` or `float`. Beside a typed value it takes that type: `a + 1` with `a` an `i32` is i32 addition. Two literals: the operation is exact on the reduced fraction (`[num, den]`, never text) and the result stays `rational_number` until it lands in a typed slot. Same concrete type keeps it; two *different* concrete types do **not** lower.
- **Why:** one operator and one literal form for every numeric type, no suffixes (`1i32`, `1.0f`).
- **Seed:** folds all-literal arithmetic exactly over `i64` fractions; out of range is a clean error, not a wrap. Arbitrary precision deferred, not blocked.
- **Source:** DESIGN.md l.166

### No implicit coercion; a numeric type applied to a value is the conversion
`i32(a)` is the conversion, per constructor, reusing token consumption; there is no `cast` identity. Comparisons resolve operands the same way and yield `bool`. Logical operators short-circuit.
- **Seed:** `not` takes a bracketed operand, a parsing shortcut, not a keep.
- **Source:** DESIGN.md l.166

### `-` before an operand is negation: one identity that reads its left side
A constructed operand at `tape[-1]` makes it subtraction; anything else, prefix. `a-1` is subtraction; `-3` folds into the literal (the literal's constructor reads the `-`). One identity has one rank, the additive one, so `-y * 3` is `-(y * 3)`, the same number as `(-y) * 3` for every arithmetic operator the seed has.
- **Why:** `-y` is what readers write; `0 - y` was a workaround. Deciding arity from neighbouring cells is what the eager-segment model gives every constructor, so no second identity.
- **Ruled:** 5 September 2026 (spec silent; the sketch had `-` always binary).
- **Seed:** rejecting `-y * 3` is a bug, folded into #59.
- **History:** first worded "binds tighter than the binary arithmetic operators"; corrected 5 Sept, since one rank cannot do that.
- **Source:** DESIGN.md l.166

### Integer `/` and `%` are total and saturate; float `%` does not exist
`x/0` gives MAX for a dividend ≥ 0, MIN for a negative one (`0/0` is MAX; unsigned has only MAX). `x%0` gives MAX. Signed `MIN/-1`: MAX for `/`, `0` for `%`. Float `/` is IEEE; float `%` is rejected at parse. This is the base tier; the refinement tier makes a nonzero divisor a compile-time obligation where opted in. The two layers fit together.
- **Why:** a loud value is easier to find than a silent `0`, the integer version of float infinity; division by zero cannot be ruled out statically in general. Sign-aware keeps the `±inf` analogy honest.
- **Ruled:** sign-aware 30 August 2026.
- **Source:** DESIGN.md l.166

### `«…»` is always a plain value; `#` is the one comment constructor
A string is never a comment by position. Prose is marked: `#` takes a following `«…»` or the raw text to end of line (sugar for the string form) and builds a **comment node** over the same text substance. One token; no line/block/doc zoo. The node is real, reflectable graph structure among the code, so docs come out by graph query. It is void-valued: evaluating it yields nothing, and value rules (trailing expression, tail commitment) read through it, so a trailing `# done` never becomes a scope's value.
- **Why:** no value rule has to guess what a bare string means.
- **Ruled:** July 2026, in discussion.
- **Rejected:** a docstring convention: at an untyped tail, `x := ( 40 + 2, «a» )` cannot tell doc from value.
- **Seed:** builds the comment node over `string` in both forms: `# raw text` to end of line, and `# «…»` bounded by the string (may span lines; anything after `»` is live code). `#` becomes a graph-resident constructor at self-hosting.
- **Source:** DESIGN.md l.168

### Strings nest by depth; only five escapes; `«…»` is the only spelling
The `«` constructor counts `«`/`»` and `{`/`}`, so a string in an interpolation in a string needs no escape. Escapes: backslash before a character the string scope lexes, or before backslash: `\{`, `\}`, `\«`, `\»`, `\\`. All else is raw. No ASCII `"…"` anywhere (files, REPL, command line); typing `«»` is a keyboard matter.
- **Ruled:** nesting and escapes 30 August 2026.
- **Source:** DESIGN.md l.168

### Pointer type is prefix `@T`; dereference is postfix `x@`; `&x` is address-of
`@i32`, `@@i32`, any type (`@point`, `@dyad`). `p@`, `p@.x`, `p@@`. `&x` takes the address of a storage-backed place (`&p.x`). The `@` family is raw, unchecked addresses, all the seed has at v0.1.0; the borrow checker's `&T`, `&mut T` come later on top: same addresses plus proof of safe use.
- **Why:** the pointer is what the user meets first, so it reads first. Postfix deref reads in evaluation order where prefix needs brackets (`(@p).x`). A dereference can never start an expression, so `@` after a value is always deref and `@` opening a type position is always the pointer type: no ambiguity.
- **Ruled:** July 2026, in discussion; replaced an earlier postfix pointer-type spelling.
- **Source:** DESIGN.md l.170

### The dyad describes itself down to one fixed point
`type` is `@dyad`, not a pointer to a classifier primitive; `value` is `@void`. Reading recurses through types and ends at the one dyad whose type is itself, whose layout is the seed's only built-in knowledge.
- **Why:** "works as a type" is a checked invariant, not a field constraint, since the substrate cannot state it.
- **Source:** DESIGN.md l.172

### Source becomes runnable by interpretation, one token at a time; compilation is opt-in
No separate parse-then-run step. Scopes marked `backend cranelift (...)` are lowered before they run; the rest is interpreted, and hot regions may be promoted to compiled form (seed: both later work, see ›The seed compiles only when told‹). Compile-time evaluation is ordinary interpretation done earlier: `comptime` forces it where it matters (a result baked into the artifact); usually it is inferred (the compiler interprets what a compiled scope depends on). It runs without I/O (effects are capabilities, ›The type strata and verification‹), so builds are reproducible; runtime code keeps I/O. Exception: `print` may write output at comptime (›The command line is Logos source‹).
- **Why (exception):** output reads nothing back, so the build stays reproducible.
- **Ruled:** `print` exception 24 September 2026.
- **Source:** DESIGN.md l.174

### A type's `parse` is unrestricted code
Parsing is Turing-complete; surfaces as irregular as natural language are in scope. A declarative consumption signature, if a type carries one, is an optional fast path the recognizer can merge and index, never a ceiling. The generic prologue (bounds, this-token-is-me) is installed by `type`'s own `parse` into every type it builds, by reference to one shared checker, so the contract is visible graph structure. Core identities, hand-built by the seed, are correct by construction.
- **Source:** DESIGN.md l.174

### A parse works on the parsing tape and the growing graph
Both exist at every source index. The tape is the frontier: dyads reduced but not final, and tokens still to take, lexed lazily (reaching a position pulls the next token), so lexing and parsing are one pass the constructors drive. A constructor reads forward for its tokens and back for its left context. When `parse_rank` guarantees a cluster takes no more tokens, it is final: it leaves the tape and attaches at the current place in the graph. Earlier dyads decide the next places: `if` opens slots for condition, body, optional `else` body, and stays incomplete until they are placed.
- **Source:** DESIGN.md l.174

### A constructor edits the tape in place; the driver decides only *when* constructors run
The tape has `insert`/`remove`: a constructor may splice tokens in or drop upcoming ones before they lex. That is the whole macro and custom-syntax mechanism, not a macro sublanguage. A constructor hands nothing to a driver: it replaces its own token with its dyad or another token, splices out neighbours it took, inserts tokens elsewhere. The driver decides when (by `parse_rank`), never what. How a constructor signals it finished: ›The constructor sets `tape.is_constructed[0] = true` itself‹.
- **Ruled:** July 2026, in discussion.
- **Source:** DESIGN.md l.174

### The tape is Logic Graph with a string extension
A constructed cell is already a graph dyad. An unconstructed cell points at its candidate identity's binding (7 Sept 2026), its span in the derived source map. The string is the source not yet lexed. So reflecting on the tape needs no second metalanguage; positions, `insert`, `remove` are ordinary identities (the sketch's `parsing_tape`) for self-hosted constructors. The tape is throwaway: source and graph persist, a re-parse rebuilds it.
- **Ruled:** July 2026; wording amended 2 September 2026.
- **History:** `token` as a wrapper identity superseded 2 Sept 2026: an unconstructed cell points at the identity itself.
- **Source:** DESIGN.md l.174

### The driver lexes a segment whole and builds it highest `parse_rank` first
The eager-segment model (›The scope's constructor is the driver‹). Higher binds tighter; associativity breaks ties.
- **Ruled:** 30 August 2026.
- **History:** replaced one-token-lookahead shift/reduce. "Operator precedence" gave way to the slot name `parse_rank` 10 Sept 2026.
- **Source:** DESIGN.md l.176

### `parse_rank` is one `f64` on one shared axis, and may be relative
Fractions let a new operator slot between two others without renumbering. Relative: `share parse_rank = mul.parse_rank - 1`, ordinary comptime field arithmetic under ›Deferral is authored‹ (unresolved stays a visible node until forced), resolved by the operator's first use or the use-before-definition error.
- **Ruled:** relative ranks August 2026, in discussion; the operator is `=` (a declared slot is filled, not redeclared) 4 September 2026 (›The constructor is a field‹).
- **Rejected:** a partial order of binding groups (closed 30 August 2026). If unrelated operators should ever demand brackets, that is an ordinary check on top, changing neither the field nor the driver.
- **History:** written without `share` until 26 Sept 2026 (l.207: slot fills carry `share`).
- **Source:** DESIGN.md l.176

### A token's identity is fixed when it is reduced; brackets bound backward reach
Until reduced, tokens can be rewritten; a reduced dyad is immune. So a higher-ranked operator on the right can rewrite tokens on its left, which then reduce with their new identity. A constructor may look as far forward as it needs and rewrite behind it, only within its own scope. A bracket pair is that bound: it forces its contents into one dyad, caps backward reach, and is where the stream resynchronizes after a structural edit. Brackets always nest (`( )`, `{ }`, `[ ]`, any scope-opening construct), as the scope spine records; `,` is an ordinary token, not a boundary. Token rewriting is a within-scope, parse-time power, unchecked in the seed, later admitted only where proven terminating and deterministic. Cross-scope work uses the rewriting engine on the reduced graph.
- **Source:** DESIGN.md l.176

### Expressions end themselves; `,` stands between every two expressions of a scope
No terminator: the finality rule ends each expression. The same `,` separates fields, arguments and expressions (other languages' `;`). In the eager-segment model `,` ranks above `(` and closes a segment when found, which also puts declaration before use in a scope.
- **Why:** one rule; the reader never judges whether two steps are independent.
- **Ruled:** 26 September 2026, Thobias; one separator since July 2026.
- **History:** before 26 Sept: "between independent expressions nothing at all stands, while between dependent ones the `,` is required", which disagreed with the seed's rule (›The scope's constructor is the driver‹).
- **Source:** DESIGN.md l.178

### Newlines are whitespace; `;` does not exist
- **Why:** `,` is already the separator; a second spelling buys nothing (restated 2 Sept 2026; the older reason, constructors decide every boundary, belonged to the superseded finality model).
- **Rejected, to stay rejected:** `;`; newline-sensitive rules (Go's continuation rule, one expression per line), re-affirmed 2 Sept 2026. A bare `if` body on the next line reads only because a newline is whitespace.
- **Source:** DESIGN.md l.178

### `if` needs no brackets around a complete condition or its body
The condition is the first complete expression after `if`, the next expression the body; a `(` right after a complete condition starts the body: `if mint != ? return mint`, `if t:type == scope (…)`, `if x == 1 (10) else (20)`. A condition ending in a type that can read a value keeps brackets: `if (t == f64) y = 1`, since the type reads its right side and would take the body's first name.
- **Why:** fewer brackets for the commonest control word; the condition's end is known once it is complete (reason pending Thobias's confirmation). Brackets for types: a type reading its right side is one rule everywhere; the condition gets no exception (Thobias: "you need brackets around t == f64").
- **Ruled:** bare bodies 25 September 2026, Thobias ("brackets should not be needed"); type conditions 26 September 2026, Thobias.
- **History:** 2 Sept 2026 declined bare `if c body`. 5 Sept: any constructed cell may be a condition (`if` reads its own right side). 25 Sept: bodies decline reversed.
- **Open:** the bare-body reason awaits Thobias's confirmation.
- **Source:** DESIGN.md l.178

### `,` outranks a type's optional operand: `f(i32 3)` is one argument, `f(i32, 3)` two
`i32 3` is juxtaposition: `i32` takes `3` into an anonymous value of its type. In `f(i32, 3)` `,` is built first (higher rank), so `i32` finds nothing and, accepting that, yields itself, the type as a value; `3` stays an uncommitted literal. No special rule.
- **CONFLICT:** l.178 opens "the `,` stands between every two expressions of a scope" (26 Sept) but later says "Separation must be explicit only where adjacency would otherwise read as consumption", older wording.
- **Source:** DESIGN.md l.178

### Hosted text does not owe a single reading
A hosted clause (Latin, English) parses to data, and data is a legitimate resting state: it stands in the graph, open to recognizer, reflection and rewriting. Ambiguous recognition: the dyad carries every matching identity. Ambiguous structure: it carries the alternative sub-dyads (a parse forest as plain values). Nothing known: the text literal is the floor. Resolution is lazy, at a use site, where later information removes alternatives or the demand surfaces `?`, the universal unknown. Hosted language is for truth: a sentence denotes a proposition (ambiguous: a set of candidates); "executing" it is the proof layer applied to it, checker as function and proposition as operand, so the evaluation rule is untouched. The kernel rarely certifies such claims (›Received code is checked before it runs‹), so an empirical claim enters as an asserted axiom with provenance, in a branch of ›the ecumenical proof system‹, where contradicting testimony coexists and the coherentist layer grades it: proven, refuted, plausible to a degree, unknown. Rankings guide what to try proving and never count as proofs. The proof layer, never the parser, assigns status, lazily and revisably.
- **Ruled:** July 2026, in discussion.
- **Rejected, to stay rejected:** driver backtracking (stored branches, rollback of final dyads): finality stays guarantee-based and one-way, exploring happens before commitment (guards, in-constructor lookahead), ambiguity is represented, not searched. `undefined` as the sign of ambiguity: it mixes too-many-readings with nothing-written, which `undefined` keeps meaning.
- **CONFLICT:** l.180: "Per *Execution is function application*, a dyad whose type is not a `fn` is never run"; l.189: "otherwise, if the type carries a `run`, run that body over the node" and "`fn` is not a primitive".
- **Source:** DESIGN.md l.180

### Compilation reads frozen structure; a structural write is deoptimization
A compiled artifact is a long-lived reader of a subgraph's structure, allowed while no structural writer touches it. The rule that a `graph_mut` invalidates readers *is* deopt: the write drops the artifact and falls back to interpretation. Runtime values are unaffected; the structure froze, not the data. So interpret versus compile is profitability, not meaning, and needs no syntax: cold or unstable code is interpreted, hot and frozen code compiled. `comptime` is the one execution marker with meaning (when evaluation happens, what is baked in). The same freeze allows optimization: equality saturation needs a fixed input, so compile, optimize and extract a cheaper proven-equal variant are one operation under one condition. Editing runs only this pure analysis (no I/O); the user's program runs nothing until invoked; the language's own machinery stays compiled, untouched by user edits.
- **Source:** DESIGN.md l.182

### The seed compiles only when told: `f.compile()`
The seed interprets everything and compiles only on request: in v0.1.0 via the `fn` type's shared `compile` member (›Execution is function application‹). Call `f.compile()` in the pass; the next call jumps to the installed code. The `backend cranelift (...)` marker and profile-driven promotion are later work on the same deopt boundary.
- **Ruled:** July 2026, in discussion.
- **Source:** DESIGN.md l.182

### Mutable code can be compiled; the reader-writer rule gates it, not `mut`
Only a region with a *continuously live* structural writer is not compiled, because the next write would deopt it. A stable `mut` region runs native. "`mut` stays interpreted" is profitability, not a ban. A compiled artifact is a read reference to code, so a structural write needs exclusive access and drops every such reader first: that drop is the deopt. Until the borrow checker makes this static, the program keeps it as a discipline.
- **Source:** DESIGN.md l.184

### The name index and `bcode` stay fresh by different means
The name index (resident trie of bindings) never mis-resolves: each binding carries its declaring scope, and one whose scope is not live is skipped, so a stale entry is inert; removal is cleanup on explicit source mutation. `bcode` is trusted and not re-checked per call, so freshness is the deopt layer's job (null the stale `bcode`, walk the body), not the writer's.
- **Source:** DESIGN.md l.184

### Beneath `fn` is one executable primitive: `@exec`, a jump address minted, never checked
`fn`'s `bcode` carries one inside its `callable` value (entry plus declared convention), nothing more. Only the machinery that made memory executable mints an `@exec` (a backend finalizing code, or a foreign declaration): fail-closed like every capability. Landing on non-executable memory is a broken upstream invariant, not a per-call test.
- **Why:** executability is a property of pages (NX bit), and no instruction can ask "may I jump here?"; the only answer is the fault.
- **Ruled:** July 2026, in discussion.
- **Rejected:** an interim `bcode {entry, arity}` wrapper: it packed the caller's knowledge into the callee's pointer.
- **Source:** DESIGN.md l.185

### The calling convention is not part of `@exec`
It lives in the code on both sides of the jump, both emitted by a backend from one abstract signature (the `input` binding's order and widths), so platform differences vanish where one compiler emits both. The signature survives sealing (a seal keeps the types read out, drops the body). Conventions are declared metadata, ordinary open-ended identities a backend renders per target, decisive only at the FFI boundary, where the foreign side froze its convention. Cost: interpretation pays a trampoline per call; co-compilation (including JIT promotion) may inline, re-register or delete the call; only real foreign code keeps an unremovable boundary.
- **Source:** DESIGN.md l.185

### `compile` never fails on an uncompiled Logos callee
The call becomes a jump into the interpreter's body-walk over the callee. Order decides the shape: `pow.compile()` then `f.compile()` gives a direct call (later inlinable); `f.compile()` alone leaves an interpreter boundary, lifted by compiling f again once the callee has code (normal deopt-and-recompile). Automatic promotion after v0.1.0.
- **Why:** an uncompiled callee is no mistake (interpreting is always valid; compile is profitability, never meaning). And `f.compile()` must mean f is compiled; silently doing nothing is the one thing a user cannot detect.
- **Ruled:** 4 September 2026.
- **Rejected:** an error on an uncompiled callee; leaving the whole function uninstalled.
- **Seed:** since 10 Sept 2026 emits the jump into the interpreter, and a second `f.compile()` lifts a boundary. Before, it failed fast and left the caller interpreted.
- **Source:** DESIGN.md l.185

### Which machine operation an operator is (`+` over `i32` → `add_i32`) is a lowering rule
Machine operations are identities (`add_i32`, `fadd_f64`), each carrying its `@exec`, unchangeable. `+` is the constructor that builds the application node. A rewrite rule, one per operator and operand-type combination, picks the operation the first time the node is lowered or run and stores it in the node's op slot; run, compile and reflection dispatch on it, no side table. Evaluate: consult the type; op slot filled, jump; else apply the rules once and fill it. Alternative implementations live in versioned scopes, not per-phase tables.
- **Why:** the node's type is `+` either way, which reflection and rewrite patterns read; the only question is who picks the machine op. That is a lowering, which ›Backends are identities‹ makes a rule set, so a new numeric type is new rules, never an edit to `+`.
- **Ruled:** 8 September 2026.
- **Seed:** `+` resolves at parse, the divergence to remove. Run tier follows the rule since July 2026: every native is a `callable` value `[entry: @exec, convention]` (the type of `add_i32` and of compiled fn code alike) in the dyad's op slot; the interpreter consults no table. Cranelift lowering still uses a Rust-side table until lowering is re-keyed per backend identity.
- **History:** before 8 Sept: "the parse-time constructor that resolves an application to one of them".
- **Source:** DESIGN.md l.185

### Foreign code is a declaration, and the C ABI is the common language
`extern` names a library, symbol, signature, convention. The loader resolves it and mints its `@exec` (the second allowed mint); an unresolvable symbol fails at load, never as a wild jump. Interpreted callers go through a minted thunk (one adapter per declaration), so the interpreter stays convention-blind; compiled callers emit the declared convention directly; a Logos function compiled against the foreign convention hands over its `@exec` (callbacks: same mechanism reversed). Struct layout is C-compatible (natural alignment, padding); text bridges explicitly (`«…»` is `[len][bytes]`, not a NUL-terminated `char*`). C is the whole strategy: Rust exports over the C ABI (no stable ABI of its own), C++ through `extern "C"`, so winning C wins both. Header ingestion is the parsing doctrine on `.h` files: a constructor reads C headers and emits declarations nobody writes by hand.
- **Why:** differing from the platform ABI buys nothing; C interop is essential while the library ecosystem is young.
- **Ruled:** July 2026, in discussion.
- **Source:** DESIGN.md l.187

### To evaluate a dyad, read its type: run a `fn`, run a type's `run`, else it is data
Operators (`+`, `=`), field access and control flow run this way. Type is a `fn`: run it on the operand record. Else, the type carries a `run`: run that body over the node, its fields being the frame. Else the dyad is data, read through the type's layout. A dyad typed by a function is an application. Control flow works as a function because operands arrive unevaluated, as Logic Graph: `if` runs its condition, then only the taken branch. Laziness is the substrate's default, not a special form.
- **Ruled:** `run` branch 4 September 2026, restated 16 September 2026.
- **History:** first: "Everything runnable is a `fn`", two branches only.
- **Source:** DESIGN.md l.189

### A user-defined operator is a `type` that fills its own slots, `run` included
`^ := type (…)` fills `share parse_rank = …`, associativity, `share parse = (…)` and `share run = (…)`. A node typed `^` runs and compiles as a node typed `f`: its type's `run` is reached as a call reaches a function, and `f.compile()` on such a call site compiles it. Demo: identities/power.logos; also tests/fixtures/caret.logos and the README block.
- **Why (name `run`):** Thobias, 17 Sept 2026: "everything is code so code doesn't really work"; the name must say what runs when the node is interpreted, or compiled and run.
- **Ruled:** 4 September 2026 (slot `code`); `run` 17 September 2026, Thobias.
- **Rejected:** names `body`, `do`, `eval`. A constructor emitting the operation inline as residual code: an operator is one operation with one definition; inlining copies it per site and hides it from reflection and `compile`, where a `^` node keeps the call visible. A separate `code` identity (signature, body, bcode, frame) shared by `fn` and operators: what it held is exactly a `fn` (dropped the same evening, 4 Sept).
- **History:** 4 Sept: `code = fn (a := i32 ?, b := i32 ?)` over a positional operand record; "`constructor`, `destructor`, and `code` are three slots of one shape, each holding a function". 16 Sept: `run` holds a body (next rule). 17 Sept: renamed `run`; the `fn` type's member `run` gave up its name (running a node is the evaluation rule; `f.run` is the body it runs). 19 Sept: `shared run = (…)` in the fields block, bare `run = (…)` the type's own. 25 Sept (l.203): fields block gone, `^.run` not `^.fields.run`. 26 Sept (l.207): mark spelled `share`, required on every slot fill; an unmarked fill is the checked error.
- **Seed:** the 4 Sept shape ran beside the new (#126) until #133 slice 9 deleted it, as ruled 20 Sept 2026: "to be deleted, not kept beside the new one, once the new shape runs".
- **Source:** DESIGN.md l.189

### `run` is a bare body over the node's own fields
No `fn`, no parameter list; fields are in scope by bare name (Thobias: "the body for code should have access to those fields directly without needing fn syntax"). A node's operands are its fields, declared by the author in the type body: `lhs`, `rhs` for `^`, one for a prefix operator, whatever a keyword needs; nothing given.
- **Why:** the 4 Sept form tied `a`, `b` to operands only by append order ("a and b now magically connects to the operands").
- **Ruled:** 16 September 2026, Thobias.
- **Rejected:** the body as instance lines: instance-building code runs when an instance is built, `run` when the node is evaluated. (The other half of that 4 Sept decline, parameters as instance fields, became the rule 16 Sept.)
- **History:** fields declared in `fields = (…)` 16-25 Sept (l.203); read as `this.f` 17-26 Sept (l.207).
- **Source:** DESIGN.md l.189

### A parse connects each operand to a field by name and removes the cells it took
Today's spelling (l.207, l.213): with `tape[0]` holding the type, `tape[0]:type = ^` makes it a new `^` node, fields as holes; the parse writes `tape[0].lhs = tape[-1]` and removes the consumed cells, so `^` is what remains of the three (Thobias: "the constructor should remove lhs and rhs in the tape since ^ now is the only thing being consumed since it has consumed lhs and rhs"). The same stamp builds a node of any other type (`squared`'s call of `sq`), so the constructor decides and nothing is granted in advance (›The constructor is a field‹).
- **Ruled:** 16 September 2026, Thobias; respelled 23 Sept (l.213) and 26 Sept (l.207).
- **History:** 16 Sept: `tape[0]:dyad.value.lhs = tape[-1]`, stamp `tape[0]:dyad.type = ^` (respelling ›The preview milestone‹'s 9 Sept "so that .operands is valid"). 17 Sept: `this` = shorthand for `tape[0]:dyad.value`; first write into `this` built the node (so a self-typed node needed no stamp: "the stamp said it twice"); `this` legal only in a constructor or `run` body inside `type (…)`, found by walking up the scope spine (#123); first use converted the cell ("this needs real lots of reflection on the LG"). 18 Sept, Thobias: `this` a separate fresh node placed by `tape[0] = this` (reason: placement written where it happens, no act on a first use), the write word.logos's own `parse` already made with `tape[0] = dyad (type, definition)`; it could be placed anywhere (`tape[1] = this`, `tape.insert(k, this)`) or nowhere (edit-and-leave error, #81). 19 Sept: in the instances' `parse`, `this` was the instance that appeared. 23 Sept: `:dyad` retired for `a:type` (Thobias had disliked writing `:dyad` first). 26 Sept: `this` removed (l.207). Declined 18 Sept: `value` as `this`'s name (it is the slot name in the same body, would name the block inside a `fields = (…)` line, and `tape[0] = value` places a bare value needing its type back); `node` noted as the only alternative.
- **Source:** DESIGN.md l.189

### `tape` is a word the `parse` slot declares into its body
Bound per run to the tape centred on the appearance; ordinary name resolution, undeclared elsewhere. So a slot body has no parameter list.
- **Ruled:** 17 September 2026.
- **History:** replaced the `fn (tape := parsing_tape ?) -> void` wrapper. `this` was declared the same way 17-26 Sept (in `parse`, `run`, `drop`); a helper read it as an outer name under "a call is a use of every outer name" (#125).
- **Source:** DESIGN.md l.189

### What `=` does is decided by the type of the place on its left
`=` drives its right side (as the seed's does) and builds it by the left place's type's `parse`. So `share parse = ( … )` reads the bracket as a deferred parse body, `share run = ( … )` as a deferred run body. This is ›the type defines how the value is read‹ (›Declarations are immutable by default‹) at parse time, and answers who defers the bracket.
- **Why:** Thobias: "how parse still works is by defining what = means when lhs is the type of parse. same for run".
- **Ruled:** 17 September 2026, Thobias.
- **History:** also read `fields = ( … )` as the field declaration block until 25 Sept.
- **Source:** DESIGN.md l.189

### A field is filled at run, per evaluation; its type decides how the operand is used
The node's storage is its frame. `rhs := i32 ?` evaluates the operand into an i32 (a literal molds as a call argument does). A node-typed field keeps the operand as graph (how control-flow words are written with `run`). An untyped hole `lhs := ?` takes the type of the first value written into it, so `lhs` is typed per node.
- **Ruled:** 16 September 2026, Thobias.
- **Open:** the two moments of a field (operand slot written at parse, frame place evaluated at run) are implied but not yet said in one sentence.
- **Source:** DESIGN.md l.189

### A node's output type is per node, and its parse writes it
The author declares `output_type := type ?`; the parse writes `tape[0].output_type = tape[-1]:type`; use sites and the lowering read it from the node.
- **Why:** `^` over i32 and f64 is one definition whose result follows its operands (Thobias: "the constructor has to look at the type of lhs and rhs to decide what the output type should be"). Written at build time, so a body using `^` gets its types in the one pass, no lookahead.
- **Ruled:** 16 September 2026, Thobias. Renamed `output_type` 26 Sept 2026, Thobias ("need to rename to output_type everywhere"): it holds the type of what the run yields.
- **Rejected:** one result type fixed on the type (not dynamic enough).
- **History:** 16 Sept: `output := type ?`, `tape[0]:dyad.value.output = tape[-1]:dyad.type`.
- **Source:** DESIGN.md l.189

### Each field-type combination a node is built with is its own specialization for `compile`
- **Ruled:** 16 September 2026.
- **Source:** DESIGN.md l.189

### A member stands before any body that reads it
A type body is read once, in order, no lookahead; reading a field uses its name, and names are declared before use.
- **Ruled:** 20 September 2026, Thobias: "instance needs to come before parse" (then about the fields block).
- **History:** open 18-20 Sept as "must a `fields = (…)` block precede a body that reads `this.f`?"; power.logos already had that order.
- **Source:** DESIGN.md l.189

### The constructor sets `tape.is_constructed[0] = true` itself; a tape write never sets it
A write to `tape[k]` replaces the pointer, nothing more. A constructor that finishes its cell sets the flag; one that hands the cell on leaves it false.
- **Why:** a constructor may leave an unfinished value (the array chooser places the mint, whose own `parse` the driver still runs), so a write cannot mean "done"; only the constructor knows.
- **Ruled:** 19 September 2026, Thobias.
- **Seed:** since #133 slice 6: tape write replaces the pointer alone; `tape.is_constructed[k] = true` is a native; a Logos constructor is judged by the flag; a Rust constructor's decline sets it.
- **Source:** DESIGN.md l.189

### A constructor that finds nothing to consume sets its flag and stands as itself
`t := array i32` leaves the mint constructed as the type; `a = b` leaves the array instance as itself. Driver rule: flag true, done. False and the cell holds another identity: that one's turn. False and the same identity: checked error (neither finished nor handed on).
- **Why:** Thobias: "it should rather just be itself in constructed state".
- **Ruled:** 19 September 2026 (later that day), Thobias, closing the two questions opened earlier that day.
- **Source:** DESIGN.md l.189

### `fn` is not a primitive: a function is a type in this same shape
`fn (a := i32 ?) -> i32 ( … )` is a type whose fields are its parameters, whose `output_type` the call constructor writes from `-> i32`, whose `run` is the body, with call defaults for `parse_rank`, associativity and `parse`. Every field typed and output declared, so exactly one specialization, built at definition; nothing a function does today changes. The old `fn` fields `input`, `output_type`, `body` are these three; `compile`, `bcode`, `frame` and the executing primitive belong to the specialization a node runs as. `fn` is shorthand for what `^` spells out; an operator adds only its own parse.
- **Why:** one mechanism for everything that runs. Thobias, 4 Sept 2026: "fn is just a shorthand for not needing to define constructor, precedence and associativity".
- **Ruled:** 16 September 2026.
- **Seed:** #127, after #126.
- **History:** the `fn` type was two shared functions, `compile` (lower body to `bcode`) and `run` (execute), plus per-instance `input`, `output_type`, `body` (reflectable graph), `bcode` (opaque, null until compiled), `frame` (byte size of parameters and locals per call, null when none).
- **Open:** where the `run` slot is stored (since 10 Sept 2026).
- **Open (CONFLICT already recorded in DESIGN):** does `f.compile()` reach the code of a type its body uses? l.189 says yes ("`f.compile()` on a call site whose type carries a `run` compiles that function"); the 4 Sept compile ruling declined transitive compile; the seed leaves it interpreted.
- **Open:** what an absent constructor means for a `run`-carrying type: the seed refuses it as a call form since #133 slice 9 (before, `sq2(5)` ran as a call); ›The constructor is a field‹ leaves it inert.
- **Source:** DESIGN.md l.189

### The executing primitive has two paths: jump to `bcode`, or walk the body
`bcode` present: jump. Null: walk `body`. Interpretation is the null path, so deopt is just nulling `bcode`: fail-closed. No per-call check that `bcode` matches `body` (the checker's and deopt layer's job). Only the body-walk has a reflectable live stack; compiled code is an opaque leaf (frames on the machine stack), so inspecting a running function means deopting it first. The paths are simple primitives; choosing between them is policy, not theirs.
- **History:** was the `fn` type's member `run`; lost that name 17 Sept 2026.
- **Source:** DESIGN.md l.189

### A function literal is `fn (params) -> T (body)`; the parameter list is a record type
`main := fn () -> i32 ( return 40 + 2 )`. Fields are `name := T ?` or a bare `name` (any dyad); `()` is the empty record. `input` is literally the type the brackets define. `->` is mandatory (`-> void` if nothing). The body is a `( )` scope valued by its trailing expression; `return` only exits early.
- **History:** a colon field form replaced 2 September 2026.
- **Source:** DESIGN.md l.191

### Positional arguments fill the holes in order; an expression line among them is a precondition
A member given a value at definition is not an argument; arity is the hole count. `fn (x := i32 ?, y := i32 ?, assert x > y) -> i32 (…)`: the line is residual code (›Deferral is authored‹), undecidable at definition, run per call after the frame is filled and before the body. A fault in v0.1.0; a call-site obligation once the refinement tier lands (›Pre- and post-conditions‹).
- **Ruled:** 2 September 2026, in discussion.
- **Source:** DESIGN.md l.191

### Mandatory named arguments are rejected
- **Rejected, to stay rejected:** `f(x=X, y=Y)` as the only call form: `f(x := X)` is the no-shadowing error when the caller has its own `x`, and renaming a parameter breaks every caller. Optional labels may come after v0.1.0 as sugar, not spelled `=`.
- **History:** a third reason (the callee is asleep while `(` builds, so `x = X` hits an undeclared name) was superseded 3 Sept 2026: a callable reads its own bracket (›The scope's constructor is the driver‹).
- **Source:** DESIGN.md l.191

### Brackets divide by role
`( )`: scopes, parameter lists, bodies. `[ ]`: reserved for indexing. `{ }`: interpolation inside `«…»` and `#`, unassigned elsewhere; a closed language block reaches out through `logos (…)` (›Sections, the arche, and effect identities‹, 4 Sept 2026).
- **CONFLICT:** l.191: "`[ ]` is reserved for indexing"; l.205 (26 Sept): "`array T [ … ]` ... the bracket as `elements`, typed `square_brackets`" and "`x = [4, 5]`".
- **Source:** DESIGN.md l.191

### A parameter is on the same level as the body's top lines
The body resolves in the parameter scope: `own x` on a parameter kills it from that line (›Memory and concurrency‹); a later `x := …` gets a fresh frame slot.
- **Ruled:** 3 September 2026.
- **Source:** DESIGN.md l.191

### Long form: `fn (a, b) -> c where ( … ) ( … )`
The signature is a pattern, its context one boolean; the result is named, so `where` states postconditions too. A named result is written by name in the body; bare `return` exits. The form above is the short form. See ›The proof layer‹.
- **Ruled:** 7 September 2026; named result with bare `return` 8 September 2026.
- **Source:** DESIGN.md l.191

### A scope's value is its last expression; `return` only leaves a function early
Every `( )` is a scope that produces a value: whatever its body ends with. `a := ( sum := i32 0  ...  sum )` gives `a` the value of `sum`. `fn () -> i32 ( x + 1 )` returns `x + 1`, no keyword needed.
`return X` is written only to leave a function early. It unwinds to the nearest enclosing *function* (through any loop, `if` or block in between) and makes `X` its result. So a `return` inside a loop leaves the function, not the loop. A value that is only a block's result is the block's last expression, never a `return`.
A `-> void` function, or a scope whose last form yields nothing, has no value.
Whether a `( )`'s value is used or thrown away is fixed by the construct around it (a bound block's value is used, a loop body's is thrown away). That decides where a value goes, not how it is made.
- **Source:** DESIGN.md l.193

### Operands travel on the stack, so `run` and `compile` take no arguments
`run` and `compile` each read their own fields (body, signature, code). The caller puts the call's operands on the stack and the callee's code reads them: the ordinary calling convention.
Interpreted, each operand is copied from its dyad onto the stack per execution (the interpreter's standing cost, as in any bytecode VM). Compiled, the dyad is read once at compile time and operand access is baked into machine code (immediates inlined, locals in registers or stack slots), so compiled code touches no dyad at run time and runs at native speed.
- **Why:** operands live on each thread's own stack, never in the shared graph. So execution is safe under concurrency by construction: the shared Logic Graph is unchanging code, and the changing per-call state is each thread's own stack frame. It is also what makes recursion work (each call gets its own frame), independent of and before any borrow checker.
- **Source:** DESIGN.md l.195

### A type stores its shared data once; what differs per value is stored per value
A type stores `parse_rank`, `associativity`, `parse`, `run` and `drop` once and shares them with every value of the type. The bytes that differ per value are the per-value fields. Which members are stored once is written with the `share` mark (see *The mark is spelled `share`* below).
- **Ruled:** July 2026 (in discussion; who not stated). Placement by the mark: 19 September 2026, spelled `share` 26 September 2026, Thobias.
- **History:** July 2026: `shared` was a placeholder, not real syntax; placement was to be *derived* from mutability (a never-`mut` member is the same in every value, so stored once), and `shared` would disappear once `mut` and `pub` landed, reflection reading "shared" off "immutable". Changed August 2026 (l.201): since a per-value constructor is immutable yet differs per value, placement is written structure and can no longer be derived from mutability. Changed 19 September 2026: `shared` is real syntax, the one mark of placement; reflection reads the mark. Respelled `share` 26 September 2026 (l.207).
- **History:** 18 September 2026: bringing back `shared` for a member all values share and may write was proposed and withdrawn, because the case behind it (a generic's memo of its mintings) was a plain variable, not value data. 19 September: shared-and-mutable state became a `shared mut` member of the fields block, the memo staying a plain variable beside its chooser. 26 September (l.205): the memo is `array`'s own `share` member.
- **Source:** DESIGN.md l.197, l.201, l.205, l.207

### Plain data and hand-written types are one identity, `type`
A type whose `parse` derives its layout automatically (it reads the field declarations in its scope and fills `fields` and `size_bytes`, so ordinary data needs no hand-written layout) and a type whose parsing or meaning is written by hand (operators, primitives) are **one identity, not two**. The derived layout is a default of `type`.
- **Why:** the old `struct`/`type` pair was "nearly identical by design, differing only in that a struct auto-derives what a type leaves to its author."
- **Ruled:** July 2026 (who not stated).
- **Source:** DESIGN.md l.197

### Field names live in the shared name index, not on the type
Field names are not stored on the type's record. They enter the shared name index (the trie) like every other declaration and are resolved by open-scope filtering (see *Name resolution is scope-filtered*).
- **Rejected:** a per-record `names` hashtable. Resolution is one mechanism, and a second per-type name store would shadow it.
- **Ruled:** July 2026 (in discussion; who not stated).
- **Source:** DESIGN.md l.197

### The constructors drive the tape
Each identity's `parse` does its own consumption. The seed's driver is the minimal deferred-reduction loop. The driver classifies a token from the record it already reads: the `parse_rank`, together with whether a constructor is present. Every identity has a `parse_rank`; a `parse_rank` at or above `(`'s means construction at discovery (see *The scope's constructor is the driver*).
A type may carry a *declarative consumption signature*. It stays the optional fast path that *Source becomes runnable* allows: never a ceiling, and no longer a stored byte.
- **Ruled:** July 2026; three-way classification replaced 30 August 2026.
- **Seed:** stores the metadata head in full as each identity's metadata head (not on the name's binding of *Name resolution*). The core `parse` is a `native` callable leaf (called, never read into, per the reflection boundary) under one entry signature that self-hosting will replace with Logos source.
- **History:** a schedule byte stood in until the constructors drove the tape; retired July 2026. The seed's NaN / finite / +infinity three-way classification of `parse_rank` was superseded 30 August 2026 by "every identity has a `parse_rank`".
- **Source:** DESIGN.md l.197

### `drop` runs a value's destructor; only the owning heap pointer has one in core
`drop` is the undefined null on every core identity except the owning heap pointer, the first one the seed fills. `drop x` runs `x`'s destructor. Teardown is constructor-inserted `defer`; there is no implicit destruction at scope end (see *Memory and concurrency*).
- **Ruled:** July 2026.
- **Source:** DESIGN.md l.197

### Slots are named for the moment they run: `parse`, `run`, `drop`
`parse` runs when the spelling appears on the tape, `run` when the node is evaluated, `drop` when `drop x` ends the value. `parse` stands beside `parse_rank` and `lex_rank`. The prose keeps "constructor" and "destructor" as plain English for the `parse` and `drop` slots.
- **Why:** the same naming rule that named `run` the same day.
- **Rejected:** the name *poiesis* (bringing forth from concealment into presence) for the slot. Plain words win.
- **Ruled:** 17 September 2026 (slot names); poiesis declined August 2026.
- **Seed:** #130 renamed the slots, README with it.
- **History:** the slots were `constructor` and `destructor` until 17 September 2026. The 17 September text also said the building of an instance was `this`'s job; `this` is gone since 26 September 2026 (see *There is no `this`*).
- **Source:** DESIGN.md l.199

### No climbing: a type without a parse leaves its values inert
Dispatch is one lookup with no climbing. A type without a constructor leaves its values inert on the frontier: data for neighbouring constructors to consume. This is the delimiter behaviour generalised, and it fails closed.
- **Ruled:** August 2026 (in discussion; who not stated); kept by the constructor-as-field ruling (August) and by the one-level ruling (25 September).
- **Source:** DESIGN.md l.199, l.201

### `parse_rank` and `associativity` are data, and the only triggers
`parse_rank` and `associativity` are declarative data on each identity that its constructor consults. They are **the only pattern-matching on whether a constructor runs**. A constructor that runs and finds nothing to consume declines, leaving the frontier untouched. What outranks it has already taken its operands: in `f(a)` the parentheses take a bare `a` before `a`'s constructor wakes, and before a loose `=` it simply stands aside.
- **Why:** reflection must read parse behaviour as data.
- **Rejected:** moving binding order into constructor code alone (reflection must read it as data). Richer multi-node triggers.
- **Ruled:** August 2026 (in discussion); partial-order question closed 30 August 2026: one axis, a mixing check can be layered on top (see *Elaboration*).
- **History:** whether the one global number should become a partial order of binding groups was open, closed 30 August 2026.
- **Source:** DESIGN.md l.199, l.201

### Associativity's values are the identities `left` and `right`
`left` and `right` are each of type `type`, a keyword with nothing behind it. So `^.associativity == right` compares identities.
- **Rejected:** `left_to_right` / `right_to_left` (length paid in every operator definition for a distinction every operator table in the world already makes with one word). A bool (`right_associative = true` says nothing to a reader of a definition).
- **Ruled:** 9 September 2026.
- **Source:** DESIGN.md l.201

### `parse_rank` and `lex_rank`: two orders, two words
`precedence` is respelled `parse_rank`, to pair with `lex_rank`. Both are the same kind of number on two axes, higher goes first on both: `parse_rank` orders construction within a segment, `lex_rank` orders which spelling the recognizer takes at a text position. The word *precedence* is gone wherever it named the slot; it stays only inside quotations.
- **Why:** one word for two orders would let a change to how `*` binds change how text is cut. `rank` over `pri` because the core spells words in full and speaks them.
- **Ruled:** 10 September 2026.
- **Source:** DESIGN.md l.201

### `lex_rank` belongs to the binding, not the type
`lex_rank` ranks a *spelling* against other spellings that could match at a text position. A second name for the same identity (`pow := ^`, `x := i32`) is a different spelling with its own rank. So `lex_rank` is name data and lives on the binding, read `^:lex_rank` beside `^:name` and the gates (*The dyad's read surface*, the two-level rule).
`parse_rank`, `associativity` and `parse` stay in the identity's value, so they cross `:=`: `pow := ^` binds `pow` to `^`'s dyad, so `pow` parses exactly as `^` does and `2 pow ^ 3` is the same malformed expression as `2 ^ ^ 3`. `pow`'s lex rank is the default, set on the name afterwards with `pow:lex_rank = …` in the rare case one spelling must outrank another.
- **Why:** the three (rank, associativity, parse) are one unit: a constructor without a rank never wakes at the right moment, and a rank without a constructor wakes nothing.
- **Ruled:** 14 September 2026.
- **Open:** how a pattern identity (which has no other spelling) gets its `lex_rank` written after its declaration. `x:lex_rank = …` is to be the one way for every name; that spelling for pattern identities is what stays open.
- **Seed:** since 14 September 2026 (#122): `x:lex_rank` reads, `x:lex_rank = …` writes, the lexer ranks every match by its binding, the two fresh patterns carry theirs from the start, and the head's slot is gone. Stand-in: a `lex_rank = …` line in a `type (…)` body writes the binding of the declaration the body is the value of; the checked error where no declaration is being filled. Kept because a pattern identity has no other spelling to be written on; to be deleted once it has.
- **History:** `lex_rank` was listed among the slots `type` declares until 14 September 2026. The open question "how a definition writes `lex_rank` and a gate from inside the right side of `:=`" was closed for gates 15 September 2026: a gate stands left of `:=` beside its name (*Declarations are immutable by default*).
- **Source:** DESIGN.md l.201

### A slot is filled with `=`; `:=` declares a member of your own
`parse_rank`, `associativity`, `parse`, `run` and `drop` are places `type` declares for every type it builds. A type body fills them with `=`. So inside `^ := type (…)` the body writes `share parse_rank = *.parse_rank + 1`, `share associativity = right`, `share parse = (…)`. A `:=` on a slot name is the ordinary no-shadowing error.
- **Why:** the two operators keep one meaning each: `=` writes what exists, `:=` introduces what does not.
- **Ruled:** 4 September 2026. `run` joined 16 September 2026. `share` on slot fills: 26 September 2026 (l.207).
- **History:** a further reason was given for the fields block: `^.parse_rank` probed the `fields = (…)` scope before `^`'s own bare members, so a redeclared member would be found second or never (block gone 25 September 2026). `:=` spellings of these slots in earlier sketches were respelled. `fields` (spelled `value` 17 to 19 September, `instance` 19 to 23 September) was a slot too, until 25 September 2026. l.201 wrote the fills without the mark (`parse_rank = …`).
- **Source:** DESIGN.md l.201, l.203, l.207

### Slot words are known only inside a type body
The slot words are places `type` declares, and nothing at the root. Outside a type body a slot word is an unknown name, so `run := 5` there is an ordinary declaration, not the shadowing error. A lookup finds the innermost.
A slot fill names no type, so `share parse = (…)` has no right-hand `parse` to meet the slot.
In a type body, `b = 3` is the checked error whatever `b` names, an alias of a slot word included: "you cannot write b inside a type body".
- **Why:** "they cannot be known everywhere since there is e.g. a different parse inside type and instance" (Thobias). One word, two places, and the position decides, as for every field.
- **Rejected:** filling a slot through an alias.
- **Ruled:** 19 September 2026, Thobias.
- **Seed:** the root identities of #133 slice 4 were undone 20 September 2026: a type body declares the six slot words into a scope of its own around the body, closed with it.
- **History:** earlier 19 September 2026: the slot names were the core words, known everywhere, `=` with one on its left filled the innermost definition's slot, and outside any definition it was the checked error. Superseded the same day.
- **Source:** DESIGN.md l.201

### `drop` is one word
`drop`'s parse looks to its right. If `=` stands there, it is the slot being filled; anything else, and it drops what follows. So `drop x` works in every scope, a collection's own drop body included.
- **Why:** "when it is not assigned something it is rather dropping RHS … its still only one drop word" (Thobias).
- **Ruled:** 19 September 2026, Thobias.
- **Seed:** `=`'s constructor, not `drop`'s, takes a lone `drop` to its left as the slot's name, since `=` constructs at discovery before a prefix keyword's turn (divergence). #130 closed into #132.
- **History:** 17 September 2026: a bare `drop = ( … )` in a type body named the slot, the fields-block binding being "the innermost `drop`" the lookup meets, never the `drop x` statement (#130 ambiguity). Superseded 19 September 2026: there is no second `drop` to be innermost.
- **Source:** DESIGN.md l.199, l.201

### `[…]` builds itself; the collection consumes it
`[…]` constructs **itself**: its inside is parsed once, generically, into a passive node carrying the index. The collection's constructor consumes that node and emits the bounds check as leftover code where the index or the size is not known statically (see the deferral ruling).
- **Ruled:** August to early September 2026 ("same discussions").
- **Source:** DESIGN.md l.201

### Expression lines in a type body are construction code (status after 25 September unclear)
Original wording: "An *expression* line in a `fields = (…)` scope is instance-construction code: undecidable at definition, where the per-instance places are holes, it runs per instance once the fields are filled, a construction invariant, the frame's precondition of *A function's surface* generalized."
- **Ruled:** 2 September 2026.
- **Open:** DESIGN does not say what became of this rule when the fields block went (25 September). The seed now reads prose lines in a type body (see *A type body describes one level*).
- **Source:** DESIGN.md l.201

### Fields are read with a dot outside their scope
A field is never a bare name in surrounding code; outside its scope it is reached with `.`.
- **Rejected:** `self.` and `child.` prefixes. Structure carries what a mark would have said.
- **CONFLICT:** l.201 says "a field's declaration is checked only against its siblings (the seed's check against the outer stack is a bug)", with `x := 1, p := type (fields = (x := i32 ?))` legal. But l.201 also says a type body's declarations "obey the no-shadowing rule like any block's (`y := 1, g := type (y := 3)` is the error)", and its 19 September note keeps that for the fields block ("no-shadowing applies as before"). Since 25 September (l.203) fields and body lines are the same lines, so the two cannot both hold. l.207 sides with no shadowing: "a type body written inside another must not reuse the outer one's names … remember logos uses non shadowing like zig".
- **Source:** DESIGN.md l.201, l.207

### Still to be written in type.logos; reflection of fields is open
Direction (23 September 2026, Thobias): type.logos is not done. Every field under the `type` scope must be expressed there, and the definition of `type` must define what `run` and `drop` do, much as `parse` is, or the language is not self-contained.
- **Open:** what a type exposes to reflection about its fields (spelled then `^.fields.length`, walking `^.fields` with `for`, a field's binding as the walked item). The website's arity loop (content/showcase, power reflection) is his intent, not yet a ruling.
- **Open:** the type's own `drop`. On 23 September the question was asked for `run` and `drop` and the answer named `run` alone, so the type's own `drop` was left open. DESIGN does not say whether the one-level ruling (25 September) settled it.
- **Source:** DESIGN.md l.201

### Sketches for type bodies
identities/array.logos, identities/power.logos, identities/fn.logos. fn.logos fills no `run`: each function's body is its own value's.
- **Seed (per l.201, 19 to 24 September):** `run` is installed on the type as the body's lexed cells, with the functions constructed from them per field-type set kept beside them (since #133 slice 8; the `fn` form deleted since slice 9). A `share` member is read through the type alone, `g.y`, not `inst.y` (#116); Thobias, 23 September: "the shared fields are also readable from per instance its just that its the same across all instances".
- **Source:** DESIGN.md l.201

### A type body describes one level
A type body holds its slots and its values' fields together. `mut size := u64 0` and `share element_type := t` stand directly in the body, `share` marking one place and an unmarked member a place per value. `share parse = (…)`, `share run = (…)` and `share drop = (…)` fill the slots with `=`. Reads have no middle step: `^.run`, `point.dims`. `run` is a slot of the type, holding its values' body.
- **Why:** the type's own lines and the fields block held the same slots (`parse_rank`, `associativity`, `parse`, `drop`), so every type described two levels in one body, while a type needs only one `parse`.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** whether `x[i] = …` in user code writes into the array. That needs `at` to give a place, not a copy.
- **Seed:** since 26 September 2026 (#150): a type body is read as one list of fields, share members, slot fills and prose; a type stores one parse, rank and associativity; a value that appears wakes its type's parse; a node a Logos parse placed for its own type is not woken again; `^.run` and `point.dims` read through the type; the fields-block errors are gone. Prose before a field or member is read past; before a slot fill it is kept beside it. (#150 made a bare slot fill legal and `shared parse = (…)` the checked error; that rule is superseded by the `share` ruling, see below.)
- **History:** see *History of type-body layout* at the end of this part.
- **Source:** DESIGN.md l.203

### A value never has its own parse; the only parse is written in a type body
A type standing on the tape (`array`, `if`) runs the parse in its own body. Any other value runs the parse in its type's body. Every type is made by `type (…)`, so `x:type:type == type` for any identity `x`: one type level for everything. A constructor makes a node a value of a type by stamping it (see `tape[0]:type = T` below); being a type is an act, not a property granted in advance.
- **Why:** parse code lives in one place per type, and no value carries any.
- **Rejected:** reading `array`'s parse as `type`'s (since `array` is a value of `type`). It brings back the metaclass tower: a type of its own for every keyword, no `array_type`, no singleton type per keyword. Also rejected earlier (August 2026): unique syntax (an `import`, a `#`) on a singleton type of its one identity, the Smalltalk-metaclass shape, and any per-identity override slot.
- **Ruled:** 25 September 2026, Thobias.
- **History:** see *History of type-body layout*.
- **Source:** DESIGN.md l.199, l.201, l.203

### The array is two levels, not three
`array` takes any type and leaves the type of its values, `array i32`, one per element type (the memo of *The chooser's mintings* unchanged). That type's one parse builds a value when the type itself stands before a `[…]` (`tape[0]:type == type`, the cell holding the type until the parse stamps it) and reads an index when a value does.
- **Rejected:** a middle layer, a mint that builds and a second type for its values. The name `array i32` can name only one of the two, and both the building and the `⊆` check need it.
- **Ruled:** 25 September 2026, Thobias.
- **Source:** DESIGN.md l.203

### Elements are built by assignment
The list's type counts the n lines of the bracket, allocates n places, and assigns each, `(ptr + i)@ = elements.dyads[i]`. `=` constructs each by the element type's parse (what `=` does is decided by the type of the place on its left). So `array (array i32) [[1, 2], [3, 4]]` builds each inner line as an `array i32`, no special case. A bare bracket has no type yet, so its lines are read through `.dyads`, never `[i]`.
- **Why:** a parse set loose on the open tape can take whatever stands next to it, while `=` hands the parse the one right side and works the same way for every type.
- **Ruled:** 25 September 2026, Thobias. The principle that the element type decides (*Each element is built by the element type*) stands.
- **History:** the tape affordances `tape.construct(T, v)` and `tape[k].dyads[i] = v` are deleted 25 September 2026, since only the array used them. The example was written `(this.ptr + i)@` until `this` went (26 September).
- **Source:** DESIGN.md l.203, l.207

### A node a parse built runs, and is never parsed again
When a type's parse places the node it built for itself, that node's next moment is its run. So `^`'s parse needs no word for a `^` node, and `n := 2 ^ 3, n + 1` runs `n`. What runs a type's parse on a value is the appearance of a value that already existed: a name or a call's result, `a[1]` and `mk()[1]`.
- **Why:** "^ doesnt need to parse a second time. it rather runs the second time" (Thobias). Parsing is how a spelling becomes a node, and a node is past that.
- **Rejected:** Claude's consequence of the same night, that every parse building a node of its own type must say what a value of it does (`if tape[0]:type == type ( … )`). This ruling made it needless.
- **Ruled:** 26 September 2026, Thobias.
- **Open:** Claude's consequence, open to Thobias: the node a parse places runs and is never parsed again *whatever its type* (not only its own type's node).
- **Source:** DESIGN.md l.203, l.205

### `fn` needs one rank: it parses once, then runs
`fn` takes its parameters, `->` and its body at its own rank, and the function it builds runs. A call is the function running, not a second parse at another rank.
- **Why:** "fn should consume the three things to its right which is the arguments -> and the body. afterwards it just runs instead of parsing a second time" (Thobias). A type has one `parse_rank` because its parse happens once per spelling.
- **Ruled:** 26 September 2026, Thobias.
- **Source:** DESIGN.md l.203

### An array expression is a node of `array`, and its run makes the value
`array T [ … ]` is an expression, as `2 ^ 3` is. `array`'s parse builds it as a node of `array` with fields: the element type (kept for reflection), the bracket as `elements`, and `output_type`, the mint `array T`. `elements` is typed `square_brackets`, since it is the bracket as written; each run evaluates its lines; it is never an array value.
The node's `run` allocates a value of the mint, assigns each line with `=` (*Elements are built by assignment*) and returns it. So each evaluation makes a new array. The value's type is the mint; the node's type seen from outside is its `output_type`, as `^`'s is.
The mint's parse reads an index and nothing else. A mint is not written before a bracket (`t [4, 5]`) but assigned one: `x = [4, 5]`, the same act as `x = 9` with a bracket of dyads where the number stood.
- **Why:** "i think array needs run as well. so it will be similar to ^ but after it runs it returns a mint array" (Thobias). A list exists only when the expression is evaluated, and a node's `run` is what the language has for "each time this is evaluated". One path for building, not a second in the mint. Element type kept: "its nice to have it there for reflection".
- **Ruled:** 26 September 2026, Thobias ("go with b").
- **Open:** how `=` takes a bracket into a place of a mint, there and for each line of a nested list: the open question *How an operation is defined for its operands* (under *Identity recognition*). Thobias: `=` should consume the mint, not the mint `=`.
- **Seed:** since 26 September 2026 (#151): `array T [ … ]` is a node of `array` whose run makes a new value each time. `array`'s parse leaves the mint when no bracket follows. `v := T ?` of a type whose values are nodes is a new empty node. An untyped field holding a bracket has its lines evaluated at each run where the node stands. A node a Logos parse placed, of any type, is not woken again by its own type, while the value its run yields wakes the value's type, so `array i32 [4, 5][1]` reads 5. Stand-in for #152: a square bracket taken by `=` into a place of a mint (`x = [4, 5]` and each line of a nested list) is built in Rust as an `array` node over it; #152 recognises a mint as a type minted while `array`'s parse runs and reads its element type from its `element_type` member.
- **History:** the name `fill` is gone, its body being `array`'s run ("dont think fill name is needed at all", Thobias). This supersedes, in *The placed call returns the mint*, the placed call `this.fill(tape[1])` and its per-run copy of `this`; the run now does that per-run work.
- **Source:** DESIGN.md l.205

### `array array i32 [ … ]` needs no brackets around the element type
`array` binds right to left. An `array` with another `array` to its left takes no bracket and is that one's element type; the outer `array` takes the list.
- **Why:** "array array i32 as its solved is actually good" (Thobias).
- **Ruled:** 26 September 2026, Thobias.
- **Seed:** done in #151.
- **Source:** DESIGN.md l.205

### `array ?` is an empty node of `array`
When `?` stands right of `array`, its parse takes nothing and stands as itself; the hole makes the node.
- **Ruled:** 26 September 2026, Thobias ("agree").
- **Seed:** done in #151.
- **Source:** DESIGN.md l.205

### Mints stay: the element type is stored once, in the mint
The element type is stored once, in the mint `array i32`, never in each value.
- **Why:** "array i32 needs to be minted so that type doesnt need to be stored in each minted instance" (Thobias).
- **Rejected:** making every value a plain `array` holding its element type (asked about and declined).
- **Ruled:** 26 September 2026, Thobias.
- **Source:** DESIGN.md l.205

### The memo and the minting are `array`'s own members
`share array_mints := hashmap type -> type` and `share get_mint := fn (t := type ?) -> type (…)` stand in `array`'s body. `share` because the memo is one place for all of `array`, never one per node.
- **Why:** "get_mints should be a shared fn inside array and array_mints should also be a field inside array" (Thobias). `array` has values now (its expression nodes), so the old reason for a plain variable, "a chooser has no instances", no longer holds.
- **Ruled:** 26 September 2026, Thobias.
- **History:** supersedes *The chooser's mintings live in a plain variable* (its reason).
- **Source:** DESIGN.md l.205

### A value holds no spare capacity
`capacity`, `capacity_bytes` and identities/next_power_of_two.logos are deleted.
- **Why:** rounding room up for growth serves nothing until an array can grow. Thobias: "delete that file completely".
- **Ruled:** 26 September 2026, Thobias.
- **Source:** DESIGN.md l.205

### There is no `this`: a type's bodies name fields directly; a parse builds its node in `tape[0]`
Inside a type's `run`, `drop` and `share` functions a field is named bare: `(ptr + index)@`, `for i in 0..size ( … )` (as *The `run` is a body over the node's own fields* first had it, "the instance fields in scope by name").
Inside a `parse` the node is `tape[0]`. While the cell holds a value of the type, `tape[0]` is that value: `tape[0].at(k)`. While it holds the type itself, **`tape[0]:type = T` makes it a new node of `T`**, and its fields are then written by name, `tape[0].element_type = t`, so nothing is left to place. Reading or writing `tape[0].f` before the stamp is the checked error, unless `f` belongs to the type the cell holds then.
A bare field name means one thing because Logos has no shadowing: a type body written inside another must not reuse the outer one's names. So in array.logos `get_mint`, with the mint's body inside it, is declared before `array`'s own `element_type`.
- **Why:** "could this be completely removed? i would say so"; "just simply write the fields directly in tape[0]"; "logos already uses non shadowing though so people should get used to creating expressive names to avoid name conflicts … remember logos uses non shadowing like zig" (Thobias). The mark said nothing structure did not already say (the reason `self.` and `child.` were declined). One spelling for the node, `tape[0]`, in every parse. On the stamp: "what about just writing tape[0]:type = array ? that should initialize it. if something tries to read or write before that … it should be an error unless the read or write matches the type tape[0] already has".
- **Ruled:** 26 September 2026, Thobias.
- **Open:** naming the value as a whole outside a parse (a `share` function returning or handing on its own receiver). Nothing needs it today; the test types that did so are rewritten in array's shape (Thobias).
- **Seed:** not yet (#153), per DESIGN.
- **History:** superseded 26 September 2026: `this` legal only inside a constructor or `run` body (17 September); `tape[0] = this` as the placement (18 September); "this should be used inside instance as well" (19 September, a body reached through an instance bound `this` to it, and in the type's own parse `this` was the fresh instance); every `this.f` spelling in DESIGN, the sketches and the seed. The stamp was first written `tape[0].type = array` (August). Kept: the per-run copy of a parse's fresh node (*The placed call returns the mint*) for a call a parse places on the node it is building.
- **Source:** DESIGN.md l.207, l.201

### In a parse, a field is always written through its cell
In a parse a field is written `tape[0].f`; a bare field name there is the checked error.
- **Why:** the cell may hold the type or a value, and its neighbours are cells too.
- **Ruled:** 26 September 2026 (Thobias, same ruling).
- **Source:** DESIGN.md l.207

### A bare call of a `share` function works on the value the calling body is about
From a `run`, a `drop` or another `share` function, a bare call works on that same value. From a parse it works on no value, so only a function that reads no per-value field is called bare there (`get_mint(t)`); per-value work goes through the cell, `tape[0].at(k)`.
- **Why:** "lets just go with the bare names for now" (Thobias).
- **Ruled:** 26 September 2026, Thobias.
- **Source:** DESIGN.md l.207

### A `share` function writes the value it is called on, and respects `mut`
In `mut m := p (1, 2), m.bump()`, a field write inside `bump` reaches `m`, whether `m` is a node a parse built or a record built by applying the type. The receiver is not a copy the way an argument is.
A `share` function that writes a field is refused on a value that is not `mut`, as `m.x = …` is. A bare field write inside a `share`, `run` or `drop` function needs `mut` on the field itself, as `m.x = …` does. Only the type's own parse fills a field through its default entry.
- **Why:** "the write should reach the caller's record so that m.x becomes 11" (Thobias). The function belongs to the value and acts on it, and the two storage shapes must behave the same, a node being written through already. A function must not get around the gate a direct write meets ("require mut").
- **Ruled:** 26 September 2026, Thobias.
- **Source:** DESIGN.md l.207

### The mark is spelled `share`, and slot fills carry it too
Every line stored once per type says so: `share parse_rank = …`, `share associativity = left`, `share parse = (…)`, `share run = (…)`, `share drop = (…)`, beside `share array_mints := …`. An unmarked line is a place in every value. **An unmarked slot fill is the checked error.** `shared` is respelled `share` wherever DESIGN names the mark, kept only inside quotations and dated records.
- **Why:** "i want to rename shared to share. one letter less. also run and parse_rank and associativity and parse and others such which is shared should also be named as shared so that no confusion arrizes"; "share is fine" (Thobias). `share` is the plain word for what happens.
- **Rejected:** `one`, `all`, `same`.
- **Ruled:** 26 September 2026, Thobias.
- **Seed:** not yet (#153), per DESIGN.
- **History:** supersedes the 26 September seed rule of *A type body describes one level* (a slot filled bare, `shared parse = (…)` the checked error). 19 September 2026: `shared` on the fill was one place, an unmarked fill a per-instance default, an unfilled slot a place per instance costing nothing until written, the layout locking at the first instance (reason then: per-instance behaviour takes the same mark as per-instance data).
- **Source:** DESIGN.md l.207, l.201

### History of type-body layout
- **August 2026, parse dispatches through the type:** an identity's `parse` ran for appearances of its *instances*, so the code elaborating `array i32 0` lived on array's type ("the C++ intuition restored one honest meta-level up"); shared behaviour lived on shared types (operator, keyword, data type; the record's kind byte lifting into real intermediate types); unique syntax lived on a singleton type per identity. Realizing it was #48's mandate. Superseded the same month by the constructor-as-field ruling (the dispatch reading and the singleton shape; its ground kept: fail-closed inertness, no climbing, parse behaviour readable as data).
- **August 2026, the constructor is a field:** `parse` was an ordinary field a type declares for its instances, and where it was declared decided everything: per-instance (each instance supplies its own; `type` declares it so, which let each type and keyword define its own parse in its own definition, `array := type ( parse = (…), … )`, an `if` or `import` a one-liner, deleting the metaclass tower), shared (one behaviour for every instance), or absent (inert). Dispatch: an appearance of X ran X's `parse` field. Superseded 25 September 2026 by *A value never has its own parse*; the deletion of the metaclass tower stands.
- **August / 30 August 2026, two directions in one body:** bare body lines belonged to the identity itself (its slots, and new members living on it, a namespace's); everything about its instances lived in an inner block. 19 September 2026: a bare `:=` line in a type body became the checked error; every member went in the block, shared ones marked. Superseded 25 September 2026: one level, members and slot fills stand in the body together.
- **The block's name:** `instance` (disliked, 2 September 2026) → `value = (…)` (17 September: it names the dyad's `.value` slot) → `instance = (…)` (19 September, Thobias: the block carried behaviour too, so `value` named too little) → `fields = (…)` (23 September, Thobias, "i like fields more": the block held fields and nothing else). Gone 25 September 2026. Seed #128 tracked the respellings.
- **19 September 2026, the block holds instances' data and behaviour:** every block started with predeclared slots (`parse_rank`, `associativity`, `parse`, `run`, `drop`, `instance`) filled with `shared … = (…)`; a bare `run = (…)` or `drop = (…)` filled the type's own slot. Reason then: `run` and `drop` are about the instances as much as their fields. Closed the 16 September open "where `run` is stored", the 18 September open on `parse := parse (…)`, and the #130 spelling. The `run` fill was held as its lexed tape until a node's field types are known (*Deferral is authored*, 20 September 2026). Superseded 25 September 2026.
- **23 September 2026, `run` not a slot of the type itself:** "remove type.run. run should only be inside instance" (Thobias); the values' body was read `^.fields.run`, `^.run` the checked error; shared members read `g.fields.y`. Reason then: nothing wanted the type's own run, and it stood as an empty slot in front of the shared fill. Seed #141 (24 September) read `^.fields.run` through a `dyad` view. Superseded 25 September 2026: `^.run` and `point.dims`, `run` again a slot of the type holding its values' body.
- **A type body as a plain scope (August):** `g := type (y := 3, z := y + 3)` saw `y` in `z`'s line, members reached `g.y`. 19 September: spelled `g := type (fields = (shared y := 3, shared z := y + 3))`. Since 25 September members stand in the body again (spelled with `share` since 26 September).
- **Source:** DESIGN.md l.199, l.201, l.203

### Plain substrate words: dyad, `.type`, `.value`
The cell is the **dyad**; its fields are **`.type`** and **`.value`**. The view read is `a:type` (see *The dyad's read surface*). *synolon* and *hyle* are retired everywhere. `logos` names the language alone.
- **Ruled:** August 2026 (in discussion; who not stated).
- **History:** the view read was first written `(dyad a).type`, superseded 8 September 2026 by `a:dyad.type`, which `a:type` replaced (l.213). `logos` first survived in three places (the ground identity's name, the definition keyword, the language's name); superseded 4 September 2026: the first two are `type`'s (*Substrate vocabulary*).
- **Source:** DESIGN.md l.199

### Text is the quote: `lex «…»` hands back the tape, unconstructed
Getting an identity *without* running its constructor (the plain `if`, the plain `:=`, a bare `)`) needs no quoting syntax: the substrate already has an unconstructed form of anything, text. The one spelling is `lex «…»`: the lexer itself, an ordinary identity, applied to a string. The string's own bounds settle what no quote sign could: `lex «(»` and `lex «(a, b)»` are both well-formed and mean what they say, a lone bracket or a whole group.
The sketch's founding pattern already is this ruling: `assign(declare(&mut logos, «:=»), ?, ?)` declares `:=` by its text.
Two things never needed a quote: reading an identity's fields and its binding (`if.parse`, `i32.parse_rank`, `if:scope`: the tight `.` and `:` run over the token before its constructor wakes; `:` since 8 September 2026), and passing an operation as a value, which wants the callable identity `add_i32`, not the `+` constructor. So `lex` is the macro layer's tool and nothing more.
- **Why:** a one-token prefix sign is ambiguous between a bracket and a group (`'(` beside `'(a, b)`), and a bracket-shaped quote can never name a bracket at all (`())` has no reading).
- **Rejected, to stay rejected:** a prefix quote sign (`'`, which is the postfix prime; `\`, which the sketch briefly tried as `\(`, `\)`, `\:`, `\:\=`, those spellings superseded) and a quote keyword (`bare`, `quote`). Each is a second mechanism for what text plus the lexer already give, and each has the arity problem.
- **Ruled:** August 2026 (in discussion; who not stated).
- **History:** the founding example was respelled 2 September 2026, `:` having left the declaration surface. l.209 writes the field read `if.constructor`; the slot is `parse` since 17 September 2026.
- **Source:** DESIGN.md l.209

### What `lex` returns: a `parsing_tape` fragment
`lex` returns a tape fragment, a `parsing_tape` value: the cells the lexer would have put on the frontier, with their flags and their spellings. Each cell points to the binding the trie resolved at the lex site (a dyad of type `binding`, which is why cells are `@dyad`; positions live in the derived source map, not in the cell). The cells are unconstructed; no constructor wakes. A single token is a one-cell fragment; a group is what a macro splices, `tape.insert(i, lex «(a, b)»)`. `insert` splices a tape into a tape.
Text that names nothing lexes to a fresh dyad with both slots `undefined`, its spelling kept tape-side in `tape.spelling[k]` (*The scope's constructor is the driver*). Not an error: it is how a constructor spells a name that does not exist yet; constructing it later is the ordinary unknown-name error.
- **Why (for `parsing_tape`):** a bare array of pointers has no tape side, so no place for the text a cell was lexed from. `lex «5»` hands back the number pattern's binding and must hand back «5» beside it, or the constructor reading `tape.spelling[0]` finds nothing; and `lex «foo»` would lose the very spelling `:=` is to declare.
- **Ruled:** stated as consequence in the August session; tape fragment ruled 2 September 2026; `parsing_tape` 14 September 2026; cells as bindings 8 September 2026; fresh dyad ruled 2 September 2026, read spelled 14 September 2026.
- **History:** 2 September 2026: the fragment was an `array @dyad`. Superseded 14 September 2026 by `parsing_tape`.
- **Source:** DESIGN.md l.209

### `lex` runs whenever it runs: no comptime/runtime split
`lex` turns any string into unconstructed identities whenever it runs, and the fragment is owned by the graph like any other value of the scope that made it.
- **Ruled:** 30 August 2026.
- **Seed:** since 15 September 2026 (#62): an identity at the rank of the raw-text consumers that reads its quote at discovery (as `regex` does) and lexes it when its node *runs*, against the scopes open at that moment (the appearance's, for a constructor: the seed's reading of "the lex site"). The runtime carries the lexer only while the parser runs something. Each run lexes afresh, so an undeclared spelling gets its own fresh dyad per run. The spelling rides on the cell, so a spliced cell keeps its text and the number pattern's constructor builds `2` from `lex «* 2»`. `insert` takes a tape; the single-cell insert is gone. Stand-ins left: `lex` reads only its quote (a string value has no storage to read yet); a `tape[k]` read past the frontier at run time stays the checked error; a spliced cell's error caret falls where the fragment's offset lands in the source, the derived source map being unbuilt.
- **Source:** DESIGN.md l.209

### The scope's constructor is the driver

The "driver" is not special machinery and not the tape. It is the constructor of the enclosing **scope** identity: `(` building its `scope` value, and its kin. A scope's constructor is called by its own enclosing scope when its opener appears, so the drive recurses by nesting. The root scope is run by `import` over a file, and by the command line over its one line. The tape runs nothing: it only lexes and remembers.
- **Ruled:** August 2026, in discussion.
- **Source:** DESIGN.md l.211

**Tape**

### The tape is data with four affordances
The tape has exactly four affordances: `tape[k]` (a read lexes lazily, on demand), `is_constructed` per cell, `insert`, `remove`. Beside the flag it keeps a second per-cell fact, `tape.spelling[k]`.
- **Ruled:** August 2026 (four affordances); `tape.spelling[k]` 14 September 2026.
- **Seed:** the seed also has `tape.recenter(k)` as a native (see Seed status).
- **Source:** DESIGN.md l.211

### A cell is a pointer to the binding the trie resolved
A cell points at the binding the trie resolved. The binding is itself a dyad of type `binding`. `lex «array»` gives a one-cell tape fragment whose cell points at `array`'s binding. There is no `token` wrapper: `token` is not an identity, and the sketch's `token` type is superseded.
- **Why:** one read surface; no wrapper type between a spelling and what it names.
- **Ruled:** 2 September 2026; binding as a dyad of type `binding` 8 September 2026 (*The dyad's read surface*).
- **History:** `lex «array»` yielded an `array @dyad` until 14 September 2026 (see *Text is the quote*). 7 September 2026: a resolved cell was read as `tape[1]:scope`, `tape[1]:gate`, and its dyad as `tape[1]:dyad`; a constructed cell answered `:dyad` (its own dyad) and `:scope` (the tape's scope), under the path rule of *Meta-navigation* (8 September 2026). The `:dyad` read was later retired for `a:type` (see *The dyad's read surface*).
- **Source:** DESIGN.md l.211

### A constructor never writes through an unconstructed cell
An unconstructed cell points at the identity itself. So `tape[0].type = array` in the sketches means "replace the cell's pointer with the node the constructor built" (the "replacing its own token with the dyad it built" of *Source becomes runnable*), never "change the identity".
- **Why:** writing through would reclassify the identity for the whole program. The binding's gate denies that later (7 September 2026); nothing denies it in ungated v0.1.0.
- **Ruled:** 2 September 2026, stated as consequence.
- **Source:** DESIGN.md l.211

### `is_constructed` is the tape's own list, read `tape.is_constructed[k]`
The flag lives on the tape, as a linked list parallel to the cells, never as a field of the dyad. The cell's own constructor writes it `true` when it has built something. A write to `tape[k]` never sets it.
- **Why:** keeps the dyad's read surface at two fields. The spelling `tape.is_constructed[k]` says exactly where the flag lives. `tape[0].is_constructed` would be a `.` on a dyad with no such field, and a "cell view" would be a second value type for one bit.
- **Ruled:** list 2 September 2026; spelling 9 September 2026 (closing the open point); written by the constructor 19 September 2026.
- **Source:** DESIGN.md l.211

### `tape.spelling[k]` is the text a cell was lexed from
The spelling is the tape's second list, parallel to the cells like the flag. It is how a constructor learns an appearance's text: `:=` reads `tape.spelling[-1]` to declare a fresh name; the number pattern's constructor reads `tape.spelling[0]` to build its value. A cell nothing lexed (built or spliced by a constructor) answers the empty text. The text stays readable after the cell is constructed.
- **Why:** it is the tape's fact, not the binding's: the number pattern is one binding for every `5` and `42`, so the binding can only say what its own spelling is (`x:name`), never what matched here. Nothing outside a constructor needs it, since after construction the text has become the value or the name. The empty-text answer lets a constructor test "was this cell lexed?" with one read and no fault, in a preview that has faults only.
- **Ruled:** 14 September 2026.
- **Seed:** #121, 14 September 2026 (the seed's span already kept the text readable).
- **Source:** DESIGN.md l.211

### The tape is a cheap view with a movable center, over a doubly linked list
A constructor gets the tape as a view: `tape[0]` is its own cell, negative offsets its left context, positive its right. Passing a tape re-centers a window over the same cells; nothing is copied. The tape is a doubly linked list whose center is a node, so re-centering is a pointer move and `insert`/`remove` at the center are constant-time. `tape[k]` walks k links: an index meant for the small offsets a constructor reads.
- **Why:** constructors make many splices; a list makes each one cheap.
- **Ruled:** direction August 2026 (the sketch's `doublylinkedlist dyad`); built as a list, not a vector stand-in, 9 September 2026.
- **Seed:** `ParsingTape` is an arena of linked nodes, the center a node.
- **Source:** DESIGN.md l.211

**Lexing and fresh spellings**

### An unknown spelling lexes to a fresh dyad with both slots `undefined`
The cell holds a pointer to the fresh dyad; the spelling is kept tape-side. `:=` fills that dyad and enters the spelling into the trie at declaration (`key := ?` of *Declarations are immutable by default* binds exactly such a dyad). A fresh spelling nobody declares is the leftover-cell error at the segment boundary.
- **Ruled:** 2 September 2026.
- **Source:** DESIGN.md l.211

### Unknown spellings are two pattern identities
The trie holds patterns beside literals. The two fresh-spelling patterns are identities of the language start, beside the five names: a word, `[A-Za-z_][A-Za-z0-9_]*`, and a symbol run, `[^A-Za-z0-9_\s()\[\],«»#]+`. Each builds the fresh null-slotted dyad. Numbers keep their own pattern, since `1.05` split at the dot loses its zero. `«` and `#` read their own extent and need no pattern.
- **Ruled:** 10 September 2026.
- **Seed:** the letters-and-digits identifier scanner and the word-boundary check are divergences (#110).
- **History:** 9 September 2026 (superseded 10 September): a fresh spelling reached from where the trie stops matching to the next whitespace, bracket, `,`, or start of a known spelling. So `^ := …` declared `^`, `a:=1` lexed `a` then `:=`, `x^2` with `x` declared lexed `x`, fresh `^`, `2`; a first appearance wanted spaces. Reason then: the lexer must bound an unknown spelling before it sees the `:=`; whitespace, brackets and `,` are the only structure; stopping at a known spelling keeps `a:=1` and `f(x)` readable.
- **Source:** DESIGN.md l.211

### Every spelling has a `lex_rank`; rank first, then length
At one position, the highest-ranked candidate that matches wins, whatever its length. Equal rank falls to longest match. The two fresh-spelling patterns rank below every declared spelling by default, so they match only where nothing declared does. One boundary rule sits beside rank and length: a declared spelling whose match would end between two word characters is no candidate. So a word is indivisible, a symbol run is not.

Examples: `ab2` is one fresh name even with `a` declared. `a^2` is `a` `^` `2`. `@@i32` is `@` `@` `i32` (`@` outranks the run). `x=-1` is `x` `=` `-` `1` with no rank set anywhere, since `=-` is nobody's spelling. `..` is its own identity, `regex «..» := type (…)`, and wins over `.` by length at equal rank, as `:=` wins over `:`.
- **Why:** rank before length is the only order where a catch-all can exist without swallowing `x^2` or `@@`. Longest match at equal rank is the rule the trie already had. The word-boundary rule is needed because rank alone would let a higher-ranked `a` cut `ab2`, while the greedy word pattern never ends inside a word.
- **Ruled:** 10 September 2026; `lex_rank` lives on the binding, `x:lex_rank`, 14 September 2026 (*The constructor is a field*).
- **History:** first form, 10 September 2026, same day: only patterns ranked, and a literal met a pattern by length. First wording also claimed the boundary rule fell out of longest match; corrected the same day (under rank-before-length it does not).
- **Source:** DESIGN.md l.211

### Two declared spellings of equal rank that can match the same length are a definition error
This is not something the lexer arbitrates. It is an inconsistency in the definitions, the checked error raised when the second one is declared: one must rank above the other. The seed registers no ranks, since nothing in the core has that shape. Hosted text keeps its own rule, every reading as data (*Hosted text does not owe a single reading*).
- **Why:** a silent pick by declaration order would be a ruling nobody can check. A definition-time error is seen once by the author; a lex-time error would show only on text that happens to hit both.
- **Ruled:** 10 September 2026.
- **Source:** DESIGN.md l.211

### Finding the highest rank is cheap
The byte walk already narrows literals to the one that can match (no shadowing, one binding per spelling). Patterns are few, bucketed by first byte, and tried in rank order until one matches.
- **Ruled:** 10 September 2026.
- **Source:** DESIGN.md l.211

### A pattern identity is declared `regex «…» := type (…)`
`regex` is an ordinary identity that reads its own quote, as `lex` does, and gives a recognizer, which `:=` enters into the trie. It is the third reader of the one quote, beside text (`«…»`) and code (`lex «…»`), the same shape as `lex «)» := logos (lex «)»)` in *A language is a section with an authored start*. One door serves a literal (`regex «..»`) and a pattern (`regex «[0-9]+»`). The quote's escapes (`\«`, `\»`, `\{`, `\}`, `\\`) apply before the pattern is read, so a literal backslash in a pattern is written twice. A hosted language section fetches `regex` like any name; only the two core patterns are authored into the start.
- **Why:** a pattern cannot lex as its own name, so some word must introduce it. Naming the class leaves room for the other recognizer kinds of *Identity recognition*. A pattern that is a pure literal splits onto the literal path anyway (the seed's regex splitting), so no separate quoted-name form is needed.
- **Ruled:** 10 September 2026.
- **Source:** DESIGN.md l.211

**Brackets and application**

### `[` is `(` in square brackets, and lands a `square_brackets` cell
A `[…]` is parsed as any bracket and any expression. The cell a `[` lands is a value of its own identity, `square_brackets`, as the cell a `(` lands is a `scope`. The tape's constructor consumes the `[…]` on its right; `tape[k]` is that read. The index inside is read as a scope's lines are: `tape[1].dyads[0]`, and `tape[1].dyads.size` counts entries. `[i, j]` gives two entries with no extra rule.
- **Why:** Thobias: "square brackets work basically exactly the same as () just that its square brackets instead". Own identity: "square_brackets should be an identity just like scope is an identity"; a constructor that consumes a `[k]` tells the bracket apart by type, `tape[1]:type == square_brackets`, which a `scope` could not be told from `a (k)`. Reading `.dyads`: the bracket reads as a scope reads, `.dyads` already exists, no new word needed.
- **Ruled:** `[` is `(` 9 September 2026, Thobias; `square_brackets` identity 23 September 2026, Thobias; `.dyads` read 25 September 2026, Thobias.
- **Seed:** since 25 September 2026 (#137), `[` lands a `square_brackets` cell whose `dyads` hold its lines. The seed's `[` still folds `t[k]` in its own constructor, a tape value having no constructor of its own yet.
- **History:** `tape[1].index`, first written in identities/array.logos, respelled `tape[1].dyads[0]` (25 September 2026).
- **Source:** DESIGN.md l.211

### `a[k]` is an application, exactly as `a(k)`
The type's parse that takes the `[k]` builds a node the way a call builds its call node: its fields hold the value and the index, and its run reads the element when it runs. `a[1]` is the call of `at` over `a` and `1`, run where it stands, at top level, in a function and compiled alike, bounds check included (the residual code of *Deferral is authored*). A call written into a tape cell is placed, not run: a cell is a node-typed place, and "a field typed as a node keeps the operand as graph" (*Execution is function application*, item 4).
- **Why:** Thobias: "a[1] would be almost identical to a(1) just that a consumes square_brackets instead of a scope. square_brackets and () behave identical, its just the name that is different." One mechanism for `(…)` and `[…]`.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** Claude's, open to Thobias: the member's name, `at`.
- **History:** the first array.logos read the parse's own locals after the parse had returned, `tape[0] = scope ( … index … this … )`; superseded by this ruling. The 25 September spelling was `tape[0] = this.at(tape[1].dyads[0])` with `shared at := fn (index := u64 ?) -> element_type ( if index >= this.size error «…», (this.ptr + index)@ )`; since 26 September 2026 there is no `this` and the mark is `share` (a value in the cell is `tape[0]`, `tape[0].at(k)`; fields named bare in the function body, see l.207).
- **Source:** DESIGN.md l.211

### A placed call keeps its tape lines as operands
An argument of a placed call that reads the tape (the index line `tape[1].dyads[0]`) is that line itself, an operand run when the call runs, exactly as `f(i)` keeps `i`. It is never its value read at parse. The parse checks only the line's type, which it knows: `tape[k].dyads[i]:type` is a name's declared type and an expression's result type, and a literal molds to the checked type. Only what the placed node cannot reach later runs as it is placed: an argument that reads the parse's own node or locals. So `i := u64 1, a[i]` is 2; `fn (i := u64 ?) -> i32 ( a[i] )` reads `i` per call, interpreted and compiled; `a[j + 1]` runs `j + 1`; an index at or past the size is "index out of range" when the call runs.
- **Why:** Thobias: "a[i] and f(i) should handle i in the same way". A name or expression has no value while the parse runs; reading one there is what made `i := u64 1, a[i]` fail.
- **Ruled:** 25 September 2026, Thobias.
- **History:** Claude's reading of the same day, "the placed call's arguments run as it is placed … so the node holds values", superseded.
- **Source:** DESIGN.md l.211

### An index may be of any integer type
`a[i]` takes an index of any integer type. One that fits the size type (`u8` to `u64`, or a literal that molds to `u64`) goes to `at` as a `u64`. A signed one goes to `at_signed := fn (index := i64 ?) -> element_type`, whose check refuses a negative value as it refuses one at or past the size: "index out of range" when the call runs. So `for i in (i32 0)..(i32 3) ( s = s + a[i] )` sums, and `a[i] = v` writes alike. A literal index still answers at parse: `a[-1]` and `a[1.5]` stay "the index type is not within the size type". The widening is the narrowing's own (*A tape read checked against a number type reads as that number*): a line checked `⊆ T` is read as a `T` in the call it places, converted when the call runs; no cast is written and none is implicit, the check being the cast. `a.at(k)` is an ordinary call and stays "these types do not match" for `k := u8 1`.
- **Why:** a loop counter is an index (`for i in 0..3` counts in `i32`), and the bounds check, not the index's type, keeps a read inside the array.
- **Ruled:** 25 September 2026, Thobias ("yes").
- **Open:** Claude's choices, open to Thobias: a second member rather than one `at` over `i64` (a `u64` index does not fit `i64`, and the size type is `u64`); `at_signed` repeating the bounds check rather than calling `at` (only a body ending in its own dereference is a place); the name `at_signed`.
- **Seed:** since 25 September 2026, in array.logos and in the placed call's read of a narrowed line.
- **History:** before, the check `⊆ size:type` was the whole rule, and `k := u8 1, a[k]` was "these types do not match".
- **Source:** DESIGN.md l.211

### A call that ends in a dereference is a place
`f(…) = v`, where `f`'s body ends in a dereference `p@`, runs the body with the call's operands as the read does (bounds check included) and stores `v` where `p` points instead of reading there. `v` is typed at parse against the dereference's type, a literal molding to it, as for `p@ = v`. So `a[k] = v` is `=` over the call of `at`, at top level, in a function and compiled alike; `a.at(k) = v` is the same write; `a[j + 1] = …` runs the index when the write runs; an index out of range fails when the write runs. A write through a parameter (`p[1] = …`) or through `b := a` is seen by every holder. A call whose body ends in anything else is "this is not an assignable place"; so is one whose body holds a `return` (it could leave with a value where the write needs an address) or a teardown of its own (it would free what the address points at before the write lands).
- **Why:** `a[k]` is an application, so its write is an application's write: one rule for every call, none for arrays. `at` already reads through a dereference, so the place exists and only `=` must reach it.
- **Ruled:** 25 September 2026, Claude's choice (DESIGN was silent), approved by Thobias the same day ("yes").
- **Rejected:** the `[k]` parse seeing `=` on its right and placing a `set(k, v)` (a parse cannot construct the right side of `=`, and a second member would repeat the check); `at` yielding an `@T` the parse dereferences (`a.at(k)` would then hand out a pointer).
- **Seed:** since 25 September 2026, `=` over such a call builds a copy of `f` whose body ends in `p` and returns `@T`, and stores through that copy's call; no array-specific code. A fault a compiled caller parks is the first one, so the null guard over the zero an interpreted callee's error left does not replace that error.
- **CONFLICT:** l.203 (26 September 2026) says: "Recorded open: whether `x[i] = …` in user code writes into the array, which needs `at` to give a place and not a copy." l.211 (25 September 2026) says `a[k] = v` writes, approved by Thobias, and is in the seed.
- **Source:** DESIGN.md l.211

### `a` need not be `mut` to write its elements
The element is not inside `a`'s binding but at the address `ptr` holds, and "a dereference starts a new path" (*`mut` is a gate on the binding, not a type*), as `m[k] = v` fills a map declared without `mut`.
- **Ruled:** 25 September 2026 (with the place ruling).
- **Open:** for Thobias: the through-gate of that path (a pointer made by `mut alloc`) is not checked by the seed ("a write through `@` is unchecked"), and array.logos allocates with plain `alloc`, whose writes that gate would refuse once checked.
- **Source:** DESIGN.md l.211

### Any expression whose type has a parse wakes that parse
A name, a parameter, a field, or any expression whose type is known at parse to have a parse wakes that parse where it stands, as the value itself does. The value there is that expression, an operand of the call the parse places, run when the call runs. So `a[1]` works on `a := array i32 [x, y, 3]`, whose contents exist only at run, and on a parameter `p := array i32 ?`. With `mk := fn () -> array i32 ( array i32 [1, 2] )`, `mk()[1]` is 2 as `m := mk(), m[1]` is; `mk().at(1)` is the same call spelled out; each run of `mk()[1]` runs `mk` anew, as `f(x) + f(x)` runs `f` twice.
- **Why:** the type decides how a value is read, whether it is known at parse or only at run, and whatever produced it.
- **Ruled:** names, parameters, fields 25 September 2026, Thobias ("go with B"); any expression 25 September 2026, Thobias ("yes").
- **Open:** Claude's reading, open to Thobias: a bracket an identity to its left takes stays that identity's argument (`f (a)` is a call of `f`), since `(` builds its bracket at discovery, before that identity's turn. Only a bracket nothing took, with a cell to its right, wakes, once the segment's other constructors have run.
- **Seed:** since 25 September 2026 (#137): names, parameters, call results and brackets wake, interpreted and compiled; `a[1]`, `b := a, b[1]`, and `f := fn (p := array i32 ?) -> i32 ( p[1] ), f(a)` work, the last also after `f.compile()`. A field read wakes too (later on 25 September; before, `this.items[1]` over a field was refused).
- **History:** the 25 September wording said "a type whose fields block fills the instances' `parse`" and "`this` there is that name's value"; since 25/26 September a type has one parse and no fields block (l.203) and there is no `this` (l.207).
- **Source:** DESIGN.md l.211

### A name standing with nothing to its right wakes and stands
`f(a)` or `b := a`: the woken parse reads `void` at `tape[1]`, takes no bracket and stands. So `b := a, b[1]` is 2, and `t := array i32` stands as the type.
- **Ruled:** 25 September 2026, closed by the `void` read (see Lazy tape reads).
- **Source:** DESIGN.md l.211

**Arrays and values of Logos-built types**

### A list's elements are filled when the code runs
The elements of `array i32 [x, y, 3]` are filled at run, like the arguments of `make(x, y, 3)`: each line typed at parse against the element type (`array u8 [1, 300]` refused where written) and evaluated at run.
- **Ruled:** 25 September 2026.
- **History:** the mint's fill loop read each line's number at parse, which refused a name among them; superseded 25 September 2026.
- **Source:** DESIGN.md l.211

### A bracket goes to the call whole
The bracket is handed over as one argument; the callee evaluates each line when it builds the array. The bracket keeps its own type: the callee reads `elements.dyads.size`, `elements.dyads[i]:type`, and after a check `elements.dyads[i]` as the line's value. Only the array's list must be square; a type of the author's own may still take a `scope` as its argument.
- **Why:** Thobias: "hand over elements as bracket arguments yes as long as square_bracket is used and not parenthesis because if its parenthesis then it should be passed on as scope argument. but really they are the same just different name." One rule for every bracket: lines are operands, and one bracket fits a fixed parameter list however many lines it holds.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** Claude's readings, open to Thobias: the parameter is a bare name (a typed `scope` parameter would refuse a `square_brackets`; *A function's surface*: "a bare `name` that accepts any dyad"); lines are evaluated in the frame the call stands in, when it runs, into a new bracket of the same kind holding their values (a number line as its value, a literal as itself), since the callee's frame holds none of the caller's names. So in `fn (x := i32 ?) -> i32 ( c := array i32 [x, x + 1], c[1] )` the lines read that call's `x`.
- **History:** 25 September: the mint's parse placed `tape[0] = this.fill(tape[1])` with `fill := fn (elements) -> this:type (…)`. Since 26 September 2026 (l.205) `fill` is gone: `array T [ … ]` is a node of `array` whose `elements` field holds the bracket and whose run does the work.
- **Source:** DESIGN.md l.211

### The array's list is written in square brackets
`array i32 [1, 2]`. A `scope` there is the checked error with the guide "the list is written in square brackets, `array i32 [1, 2]`, not `array i32 (1, 2)`". Examples written `array T (…)` in older text are read as `array T […]`.
- **Why:** Thobias: "replace () with [] for the literal list given to array T () so it becomes array T []". Reason, in Claude's words, open to Thobias: `( )` delimits scopes, parameter lists and bodies, and a parenthesis after a value is a scope argument, so a list written `(…)` would look like a scope while being data. One spelling for one meaning, and `array i32 [1, 2]` reads like the index `a[1]` it is read back with.
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** identities/array.logos since 25 September 2026, no Rust change.
- **History:** `t := array i32, x := t [4, 5]` was also valid on 25 September; since 26 September a mint is not written before a bracket but assigned one, `x = [4, 5]` (l.205).
- **Source:** DESIGN.md l.211

### A value of a type built by a Logos parse travels as a pointer
Such a value is the address of its node, eight bytes in a place. `b := a`, `f(a)` and returning it copy the address. So `b := a` shares the array, it does not copy it: a write through either is seen through both. `a`, the name the array was made into, frees it (*A filled `share drop` is the teardown the constructor authors*); `b` borrows. The type's reading rule decides this (*`:=` decides nothing about copy versus reference; the type's reading rule does*, 5 September 2026): this rule reads the node's address as `type`'s reads the identity, so "two names for one place among data exist only where written, `y := &x`" does not reach these values. A parameter `p := array i32 ?` is such a place; `p.size` reads through the address when the body runs, and `p.size_bytes()` calls the member on `p`.
- **Why:** one storage shape for these values, the node's address. It fits the 8-byte argument slot, so passing one needs no copy rule of #115.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** a record built by applying its type, `p (1, 2)`, is not such a value: it stays bytes at the type's width (the stand-in "a record-typed declared place is its layout's bytes"), and a fields-block function reading a node's fields on one is the checked error until ruled (#149). Moving those records to the pointer shape too would be the one-shape answer, recorded open, since it reaches every record's layout, compiled field reads and `@T` over a record.
- **Source:** DESIGN.md l.211

### A field of such a type holds the node's address
A field `items := t ?` with `t := array i32` holds the address of the array's node. Reading the field gives a value of `t`, so it wakes `t`'s parse like any expression of the type: `b.items[1]`, `b.items.size`, `b.items.at(1)`, `b.items[0] = i32 9` and `f(b.items)` work as on a name; `c := b.items` borrows, as `c := a` does. Writing the field stores the node `v` yields, typed at parse against `t`, never the expression (a stored expression would build a new array at each read). The field is read when the code runs, even through a node the parse already knows (a top-level `b`), since it may be written again before then; a field of any other type keeps the parse-time read of a known node.
- **Why:** from "such a value is the address of its node, eight bytes in a place", and a field is a place: one storage shape wherever the value stands.
- **Ruled:** 25 September 2026, Claude's reading, asked by Thobias to "fix this"; open to Thobias.
- **Seed:** since 25 September 2026, no array-specific code. A function holding the field read stays interpreted, as every field read does (only the per-run copy lowers); a compiled callee handed `b.items` runs compiled.
- **Source:** DESIGN.md l.211

### Each evaluation of an array expression makes a new array
Each evaluation of `array i32 [x, y, 3]` (each pass of a loop, each call of a function holding it) is a new array.
- **Why:** the contents exist only at run, so one node built at parse and shared by every run of the site would hold only the last run's elements.
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** since 25 September 2026 (#137): `array i32 [x, y, 3]` fills at run; a function that makes an array compiles (the bracket's number lines run in the compiled frame; the new bracket and the per-run copy, nodes of the store, are made by a call from machine code back into the seed; the builder runs interpreted as any uncompiled callee does). A return type spelled `-> array i32 ( … )` stops before the body's bracket (the mint's `tape[1]` reads `void`), so `mk := fn () -> array i32 ( array i32 [1, 2] )` returns the array.
- **History:** 25 September: "the placed call returns the mint": the call yielded the mint as a pointer; Claude's spelling (open to Thobias) named the mint `this:type` inside its own fields block, and a call a type's parse placed on its `this` took a per-run copy of that node, begun as the parse left it. Superseded 26 September 2026 (l.205): the array expression is a node of `array` whose run makes the value; `fill` and its per-run copy are gone. Before the sealed read, the mint lexed past the return type into the body, and a function returning an array had to name the type first, `t := array i32, … -> t ( … )`.
- **Source:** DESIGN.md l.211

### `alloc n of T`, for such a `T`, gives n cells of its address
`alloc n of array i32 ?` allocates n cells of pointer width. `p@` reads the address held as the value; `p@ = v` stores `v`'s address; `p + k` steps k cells of eight bytes. As consequence: a line `tape[k].dyads[i]` checked `⊆ T` for such a `T` reads as a value of `T` after the check; a line's `:type` is the type of what it yields, so a made value `array i32 [1, 2]` in a list answers `array i32`, not the call that makes it; a handed-over bracket evaluates such a line where the call runs, giving the callee the node's address. So `t := array i32, b := array t [own x, own y]` holds two arrays, `b[1][0]` reads through both, `b[0][1] = i32 9` writes inside the inner one (seen by every holder), and a parameter `p := array t ?` reads `p[1][1]`, interpreted and compiled.
- **Why:** the value is the address, so a cell that holds one holds eight bytes of address.
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** 25 September 2026, one test in `read.rs` for which cells load whole (`cell_numtype`), no array-specific code.
- **Source:** DESIGN.md l.211

### A value owns what its elements hold, and drops it
The outer's `share drop` drops each element whose type fills a `share drop`, then frees its own memory (array.logos: `for i in 0..size ( drop (ptr + i)@ )` before `free ptr`). `drop p@`, over a cell holding a node whose type fills `share drop`, runs that drop and empties the cell, as `drop a` does for an owner; over any other cell it stays the inert drop.
- **Why:** Thobias: "the outer array owns its inner arrays". One name owns each value, and for an inner array that name is the outer one, so the outer's drop frees the inner ones: nothing leaks and nothing is freed twice.
- **Ruled:** 25 September 2026, Thobias.
- **Source:** DESIGN.md l.211

### A list's lines move into the value built from it
A line that names a value whose type fills a `share drop` is refused: "write `own x` to move it in". `own x` moves it, `x` dead from that line. A value made in the list (`array i32 [1, 2]`, `box (1, 2)`, `mk()`) belongs to the new value from the start.
- **Why:** from DESIGN: "`=` into an owner takes only a value just made or moved" and "`b := a` and passing `f(a)` borrow". A line is an operand, which borrows, and the element is owned, so a bare name would give one value two owners and two drops. Moving it silently would end a name nobody wrote `own` on, where *Explicit heap, and no implicit destruction* ends nothing behind your back.
- **Ruled:** 25 September 2026, Claude's decision (asked by Thobias to decide from DESIGN); open to Thobias.
- **Open:** a parameter that says it consumes its argument (`own` on a parameter, not in the seed) is the precise form, recorded open.
- **Seed:** the refusal stands where a type's parse hands a bracket to the call it places, for every type written in Logos; a callee that only reads its list is refused alike.
- **History:** `own array i32 ?` as a field was found (Claude, 25 September) not to be this mechanism, missing three pieces (`own` over a non-pointer hole, `drop` over a field read, a field waking its type's parse); closed later on 25 September by *A field may be `own t ?`* and *A field of such a type holds the node's address*.
- **Source:** DESIGN.md l.211

### The element type decides how a line becomes an element
When the element type has its own parse, each line of the outer list is built as that type applied to the line; a line that already is a value of the element type is taken as it is; numbers keep their rule. With element type `array i32`, `[[1, 2], [3, 4]]` builds `array i32 [1, 2]` and `array i32 [3, 4]`, while `own x` or `mk()` of that type is taken as is. `array u8 [1, 300]` stays "an element does not fit the element type" (a number type's own parse would take a number line as a conversion, so only bracket lines are handed over). Each run makes new inner arrays, as it makes a new outer one. `array array i32 [[1, 2], [3, 4]]`, three levels, lines naming a run-time `x`, a function returning a nested array and its compiled form all run.
- **Why:** the element type decides how a line becomes an element, as a type's parse decides everywhere (*`X (…)` is one spelling, and X's constructor decides what the bracket is*).
- **Ruled:** 25 September 2026, Thobias. The principle stands after 25 September (l.203).
- **Rejected:** running a type's parse by hand (the 19 September ruling replaced it by the driver's turn); splicing the lines onto the tape (a line lies inside the bracket cell, not on the tape).
- **History:** 25 September, Claude's spelling (open to Thobias): two tape affordances, `tape.construct(T, v)` (construct the two cells `T v` as the driver does a segment, give the one node, else the checked error) and `tape[k].dyads[i] = v` (replace line i of bracket cell k); array.logos wrote `if tape[1].dyads[i]:type == square_brackets or tape[1].dyads[i]:type == scope ( tape[1].dyads[i] = tape.construct(element_type, tape[1].dyads[i]) )`. Reasons then: the construct is the driver's own act ("the driver wakes by the flag and the cell's `parse_rank`", 19 September), offered to every Logos type; the line write was the 20 September "mutate the dyad array inside scope". Both deleted later on 25 September 2026 (l.203): elements are built by assignment, `(ptr + i)@ = elements.dyads[i]`, and `=` constructs each by the element type's parse.
- **Source:** DESIGN.md l.211

### At top level, a `[…]` line of a list is not run early
Stated as consequence of *A bracket goes to the call whole*: at top level, where lines run as they are parsed, a list line that is itself a `[…]` is left for the call that takes the list. Otherwise the parse of a later line (a type's parse running first runs what stands before it) would run it early, where a `[…]` has nothing to run.
- **Ruled:** 25 September 2026, consequence.
- **Seed:** 25 September 2026, the top level's pending run skips a `[…]` line.
- **Source:** DESIGN.md l.211

**Text and interpolation**

### Strings and comments are near-degenerate scopes
`«…»` is the scope that lexes only its own two pairs, `«`/`»` and `{`/`}`, counted by depth so strings nest without escapes, plus the backslash escapes of *Text literals are plain values*. `#` is that scope to end of line. Brackets, strings and comments are one family, differing only in how far they lex and how much they build. A hosted clause (*Hosted text does not owe a single reading*) orders its words its own way.
- **Ruled:** August 2026, in discussion.
- **Source:** DESIGN.md l.211

### `{ }` is text interpolation
Inside `«…»` or a `#` comment, `{` opens an ordinary full scope whose value is turned into text and spliced where it stood: `«total {n}»`. A string is a scope that builds exactly its braces. `{ }` is unassigned outside text. The conversion is the text type applied to the value (`string(n)`; a type applied to a value is the conversion, *Numeric literals*). An interpolation in a `#` comment is not comptime-gated: the comment's text is a value that resolves when its inputs exist, and documentation tooling reads what is resolved.
- **Ruled:** interpolation August 2026; not comptime-gated 30 August 2026; the rest stated as consequence.
- **History:** extended to closed language blocks on 4 September 2026, rolled back the same day: a language block's door is `logos (…)` (see *Sections, the arche, and effect identities*).
- **Source:** DESIGN.md l.211

**Ordering: the eager-segment driver**

### Every identity has a `parse_rank`, and `(` works in two steps
Every identity nameable in text has a finite `parse_rank` (no NaN, no ±infinity class). Step 1, lex: `(` lexes its interior one token at a time. A token whose `parse_rank` is at or above `(`'s is built at discovery, before the next token is lexed: the raw-text consumers `#`, `«`, `import`, a hosted-clause opener, and `(` itself (an inner bracket recurses and lands as one built scope cell, its closer consumed by its opener). Every other token goes on the tape unbuilt. Step 2, build: at a segment boundary (the `,`, which sits above `(` and acts at discovery, or the closing bracket), it builds the cells lexed since the last boundary, highest `parse_rank` first, associativity breaking ties. Each constructor takes what its syntax needs from the fully lexed segment, left or right, any arity, no lookahead (a postfix taking two cells to its left is as expressible as an infix). At top level it runs them. Other scope kinds author their own ordering.
- **Why:** position on one axis is what keeps `# a ( in prose` and `import ./path` from being lexed as code, with no raw-consumer marker.
- **Ruled:** 30 August 2026, in discussion.
- **Seed:** converged September 2026 (#59): the loop lives in the scope's constructor, every identity has a finite `parse_rank`, a leftover cell is the error, the `,` is required.
- **History:** superseded 30 August 2026: the incremental shift/reduce-with-holding form first recorded here; the one-token lookahead of *Elaboration*; the threefold applied/holding/declined answer; the NaN/finite/+infinity classification of *A type's metadata*; the zero-or-one-cell wording.
- **Source:** DESIGN.md l.211

### Two dependent expressions in one scope need a `,` between them
`x := 5, y := x + 1`. Without the comma, `x := 5 y := x + 1` is one segment: the first `:=` drives its value to the boundary and finds the second declaration nested in it, two cells where one value must stand. The comma is the step boundary *Expressions are self-delimiting* names.
- **Ruled:** 30 August 2026; wording amended 5 September 2026 with the `:=` ruling (the July example had `x` unknown, which held while `:=` was boundary-time).
- **Source:** DESIGN.md l.211

### A constructor's outcome is read off its own cell; no holding, no re-invocation
The cell is either constructed, or declined (frontier untouched, the identity standing as its own value, like `i32` before a `,`). An error (operand missing, unfinished construct) returns as its `!` value for the driver; faults are for broken invariants (*Error handling*, amended 31 August 2026). In v0.1.0 every constructor error is a fault and the `!` return is staged (*Error handling*, 2 September 2026). After a segment is built, a leftover unconstructed cell is the checked error; the built cells in order are the scope's expressions.
- **Ruled:** 30 August 2026.
- **Seed:** #81, 15 September 2026: the outcome is read off the construct's cell by handle, whatever the center is after the call. Built, removed, or rewritten to another token is progress; an untouched frontier is the decline; a cell left unconstructed after an edit is the checked error. Nothing runs twice.
- **Source:** DESIGN.md l.211

**Readers of their own bracket**

### An identity that changes what names mean inside its bracket, or when its body runs, reads its own bracket
Such an identity sits at or above `(`, is built at discovery before the bracket is lexed, and drives the bracket's parse with its own scope open. The set: `fn` (the forced case: a body resolves in the parameter scope, which does not exist until `fn` has run, *A function's surface*); `for` and `while` (the loop variable, and the runs-again rule of *Memory and concurrency*); `defer` (runs later); `type` (its definition body); a language and a value's block that continue their scope; `conjecture` for its signature, where an unknown spelling is a hole (8 September 2026, *The proof layer*). A constructor that takes scope cells as operands sits above application, so `fn () -> i32 ( body )` is taken by `fn` before `i32`'s juxtaposition could read a conversion.
- **Why:** a bracket parsed first would already have resolved its names against the surroundings.
- **Ruled:** 3 September 2026.
- **Seed:** `fn`, `for`, `while` parse their own bodies with the barrier pushed first; this is the ruled shape.
- **Source:** DESIGN.md l.211

### `:=` is a reader too: it declares before its right side is built
`:=` sits above `(`. Built at discovery, it declares the name on its left, then drives its right side as `(` drives its interior, to the next `,` or closer (left unconsumed), until the value stands as one cell, and binds.
- **Why:** a declaration above scope makes the name exist before anything on its right is built, so `f := fn () -> i32 ( f() )` needs no forward-reference machinery: `fn`, reading its bracket at discovery, already finds `f` declared.
- **Ruled:** 5 September 2026, planning #59.
- **Source:** DESIGN.md l.211

### `if` reads its own right side
`if`'s condition is built to one cell; then the body; then an optional `else` and its body. A condition known at parse time skips the untaken bracket's text unlexed.
- **Why:** this is what *A type is a comptime value* promises, and a boundary-time `if`, finding both brackets already built, could not do it. `if` decides when, and whether, its body runs, the rule's own second clause.
- **Ruled:** 5 September 2026, planning #59.
- **History:** 3 September: "`if` keeps the plain order, its bodies being inline, once, and naming nothing new"; superseded 5 September.
- **Source:** DESIGN.md l.211

### An `if` body needs no brackets
The condition is the first complete expression after `if`; the next expression is the body; a `(` right after a complete condition starts the body. `if m != ? (1) else (2)` and `if t:type == scope (…)` need no extra brackets; `if (c) (body)` still reads. `else` takes the next expression as its body; a bare `else` binds to the nearest `if`.
- **Why:** fewer brackets for the most common control word; the condition's end is already known when it is a complete expression (reason written by the implementing session; Thobias to confirm). Thobias: "brackets should not be needed".
- **Ruled:** 25 September 2026, Thobias, reversing the decline recorded under *Expressions are self-delimiting*.
- **Seed:** 25 September 2026 (#137): a bare body is a scope holding one expression, ending at a `,`, a closer, or an `else`. An untaken bare branch is parsed and dropped, since only building it finds its end; the unlexed skip holds for a bracket only.
- **History:** before, bodies stayed scope cells, `if (c) (body)`, while a condition could be any built cell (5 September 2026).
- **Source:** DESIGN.md l.211

### A call stays a call in a condition, and `while` reads as `if`
The identity left of a `(` decides, in a condition as everywhere: a call `f(x)` stays a call, and a compared type leaves the `(` to the body. `if 1 == f(x) (10) else (20)`, `if f(x) == 1 (10)`, `if t:type == scope (…)`, `if x:type == i32 (10)` read with no extra brackets. `while i < n i = i + 1` takes a bare body as `if` does.
- **Why:** no extra brackets around a call, and one rule for every control word.
- **Ruled:** 25 September 2026, Thobias.
- **History:** earlier the same day, in the seed: a condition ended at a value, `?`, or an identity right of a comparison, so `if 1 == f(x) (…)` compared with `f` itself and the call had to be written `if 1 == (f(x)) (…)`; `while` kept its bracketed body. Both superseded the same day.
- **Source:** DESIGN.md l.211

### Anything yielding a `bool` is a condition
The condition is the first complete expression, and its result must be `bool`, whatever built it.
- **Why:** Thobias: "anything returning a bool should be accepted by if". A comparison written in Logos, a function returning `bool`, or a later operator then needs no parser change.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** to settle with Thobias: whether the shape below (a `bool` infix outside the comparisons, a type right of it) earns a field on the record.
- **Seed:** 25 September 2026 (#137): a value with a constructor (a fn value or an instance) takes a `(` after it as its call. A type left of a `(` is the compared value when it stands right of an infix its record ranks among the comparisons (below the range, above `not`), and otherwise reads its bracket, as `i32` does in `x + i32 (1) == 2`. An infix is what its record says, its first operand or field named `lhs`. `if` and `while` take any node whose function declares a `bool` output. The rank is a stand-in: whether an operator gives `bool` is known only once its constructor has built the node (a result fixed on the type is superseded, 16 September 2026, *Execution is function application*), and building it early to look, then again, is the re-invocation the driver rules out. So a `bool` infix ranked outside the comparisons, with a type right of it, leaves the `(` to the type; that shape is written `if (t ≈ i32) (…)`.
- **Source:** DESIGN.md l.211

### Inside a right-side read, a `(` after a cell that is not yet an operand belongs to that cell
Stated as consequence: `X (…)` is X's decision. `if f(x) (body)` calls `f`; `if not (c) (body)` negates `c`; `for i in n..(n + 3) (body)` ends the range there. So the body bracket is the first one after a completed operand. A return type takes no bracket: `fn () -> type ( body )` names the classifier and leaves the body to `fn`.
- **Ruled:** September 2026 (#59 convergence), consequence.
- **History:** "a bracket wanted inside a condition after its first cell is written by parenthesizing the whole condition": for `if`, narrowed 25 September 2026 by *An `if` body needs no brackets* (a `(` right after a complete condition is the body even where the cell before it could take it).
- **Source:** DESIGN.md l.211

### `for a..b` with no index is the same loop without the variable
- **Why:** Thobias: "sometimes you don't need the index".
- **Ruled:** 17 September 2026, Thobias.
- **Seed:** #129, in the seed since #133 slice 7.
- **Source:** DESIGN.md l.211

### `X (…)` is one spelling, and X's constructor decides what the bracket is
A type's constructor fills its holes from values, positional in order. A language's constructor continues its one scope: every `my_language (…)` block appends to the same body (the REPL's session model), so a name declared in one block is live in every later one until `own` or `drop` ends it (*Name resolution is scope-filtered*). A value's constructor does the same for the value: `a (x + y)` reads `a`'s fields bare (so the derived record case supplies that constructor to its values, stated as consequence). `type`'s defines. No rule decides among the three: each identity authors its own (*The constructor is a field*).
- **Ruled:** 5 September 2026.
- **Open:** whether another type's bracket (not `type (…)`) may still take a bare declaration line.
- **History:** a type's constructor also took declaration lines as new shared members, per-instance fields locking at first instantiation (*Two muts*). Struck for `type (…)` 19 September 2026: a bare declaration line in a type body was not allowed, members being declared in the fields block. (The fields block itself is gone since 25 September 2026, l.203.)
- **Source:** DESIGN.md l.211

**Scopes as data**

### A scope's lines are its one field, `dyads`
The built cells in order are the scope's expressions, an array the scope's value references, read `s.dyads[k]` and `s.dyads.size` (identities/scope.logos).
- **Why:** `self` said nothing of what the field holds; `dyads` says it (the same rule that renamed a type's block `fields` the evening before).
- **Ruled:** 24 September 2026, Thobias.
- **Rejected:** `body`, this document's prose word for the same thing, offered and not taken.
- **Seed:** since 24 September 2026 (#142).
- **History:** spelled `self` from the 2 September sketch until 24 September 2026, no reason recorded.
- **Source:** DESIGN.md l.211

### A scope's `dyads` fill as its lines complete, and hold everything
A read inside a scope still being parsed sees the lines above it: `( a := i32 1, here.scope.dyads.size )` is 1. Nothing is filtered out: prose and the `defer` teardowns a binding site adds are included.
- **Why:** every read here folds at parse, and at parse an open scope has exactly its lines so far. A filtered array would be a second list to keep in step with the first.
- **Ruled:** 24 September 2026, Thobias.
- **Source:** DESIGN.md l.211

### Reading a path runs nothing
`s := g:start.rhs` binds `s` to `g`'s block and does not run it again. A node reached by path whose reading would run code (an operator, a declaration, a block) stands as a value as its `@dyad` address; a name, literal or place stands as itself. So `(2 ^ 3).lhs` is `2` and `(x + 2).lhs` reads `x`, as `.operands[0]` does. A `mut` name bound to a reached node is not read through at parse, since an `=` may move it before the program runs.
- **Why:** a read that ran what it reads would make looking at a program change what it does.
- **Ruled:** 24 September 2026, Thobias; the `@dyad` detail stated as consequence.
- **Source:** DESIGN.md l.211

### A `:=` node's fields are `lhs` and `rhs`
`lhs` is the binding; `rhs` is the right side as written, the names every two-sided operator uses. So `s.dyads[0].lhs:name` is the declared name, and `b:start.rhs` is the `+` node of `b := a + 1`. What the seed made of the right side stays a third slot.
- **Why:** with `lhs` and `rhs` a walker needs no special case for `:=`.
- **Ruled:** 24 September 2026, Thobias.
- **Source:** DESIGN.md l.211

### A scope is changed after its close by mutating its array, never by appending text
Thobias: "you dont change scopes by inserting in the end. you rather mutate the dyad array inside scope with some written algorithm". Text an algorithm writes is lexed and built into nodes first (`lex «…»`), then written into the array as any node is. As consequence: an edit made as nodes is checked by the whole-program rule (a use of a removed declaration fails where it stands, since a use points at the binding); a rewrite by the engine is equals for equals and needs no re-check; the parse is the one checker, an algorithm choosing only whether it hands over text or nodes.
- **Ruled:** 20 September 2026, Thobias.
- **Open:** a comptime fold that already dropped its untaken branch on a value such an edit changes cannot be recomputed; text already lexed under the old index is not lexed again.
- **Source:** DESIGN.md l.211

**Tight reads and assignment**

### `:` and `.` sit above the identities that read their own right side
`:` and `.` are built at discovery like everything above `(`; their right cell is lexed lazily by the `tape[1]` read in the constructor. So a reader (`if`, `fn`, `for`, `while`, `type`) followed by `:` or `.` has its own cell taken by the read and never wakes: `if:scope` reads the keyword's binding, `if.constructor` its field (the rule of *Text is the quote*, on the axis). Above the two reads stay `:=`, `=`, `,`, the literals, and the raw-text consumers `#`, `«…»`, `import`, whose right side is text, not code; so `#:x` stays a comment, and their bindings are reached by `lex «#»`.
- **Why:** a name followed by `:` or `.` is being read, not run, and one number per identity must put a cell's readers above the cell's own constructor for that to hold. Building at discovery never required the right side built first.
- **Ruled:** 8 September 2026.
- **Source:** DESIGN.md l.211

### `=` sits beside `:=`, and returns nothing
`=` is built at discovery beside `:=`, reading the place to its left (the cells since the boundary, built to one) and driving its right side to the boundary. An assignment is an act, not a value: `a = b = c` is the error of assigning nothing, and an `=` in a value position is the statement-as-value error.
- **Why:** as recorded: the two are the one dyad's two writers, and drive alike (Thobias's own wording pending).
- **Ruled:** 8 September 2026.
- **Seed:** 9 September 2026 (#60): `:`, `.`, `@` build at discovery with their right cell lexed on demand; a reader followed by a tight read is put to sleep before it drives; the seed's right-side drives lex onto a fresh tape; `=` drives at discovery. Application and juxtaposition also build at discovery, reading their bracket or literal lazily in source order (seed rank 91, above `(`'s 90), since a tight read above them would otherwise take the bracket first: `f(2).x` and `dyad (i32, 7):dyad` read the call.
- **Source:** DESIGN.md l.211

### A cell a constructor reads from the tape arrives unbuilt
A `tape[k]` read that lexes a cell past the frontier hands it over as lexed, its constructor not run; the reader decides what it is. In `array i32 [1, 2, 3]`, the chooser `array` (at `fn.parse_rank`) gets `tape[1]` as the type `i32` itself (`tape[1]:type` is `type`: an unbuilt cell answers with its identity's own type) and `tape[2]` as the bracket. A cell the reader leaves on the tape is built in its normal turn, at the loop's next step. Only a cell whose building is its lexing is built as a read lexes it: a bracket (`(`, `[`) into its scope cell, a literal, a raw-text word (`#`, `«…»`, `import`). As consequence: this is what "application sits just below `(`" (2 September) means: a bracket is built as it is lexed, a juxtaposition is not; a juxtaposition the loop itself lexes still takes its turn at discovery (seed 91 above 90), which `f(2).x` needs, since built at the boundary its `(2)` would already be taken by the `.` as its left.
- **Why:** the tape only lexes and remembers, and `X (…)` is one spelling whose meaning X decides, so whoever reads a cell decides what it is: one rule for every reader, built-in or written in Logos. It closes the second open reason of *A parse is ranked by what it takes from its right*.
- **Ruled:** 25 September 2026, Thobias ("yes").
- **Seed:** 25 September 2026 (#137).
- **History:** 9 September 2026 seed rule: a cell lexed on demand was built at discovery when it read nothing to its left. So `i32` was built as it was lexed, took the bracket as a conversion, and failed with "a conversion takes exactly one numeric value".
- **Source:** DESIGN.md l.211

### A reader's read builds an equal-rank right-associative cell first
When a constructor's `tape[k]` read lexes a cell whose identity has the reader's own `parse_rank`, and both associate right, that cell is built as it is lexed, and the reader gets what it leaves. In `array array i32 [[1, 2], [3, 4]]` the inner `array` takes `i32` and leaves the mint `array i32`; the outer takes that and leaves `array (array i32)`, whose parse takes the list. `array array array i32 [[[1]]]` chains alike; `array (array i32)` reads alike, its bracket built as lexed. This narrows *arrives unbuilt* for this case only: `i32` sits at application's rank, below the chooser's, so it still arrives unbuilt; a cell of another rank, or of equal rank associating left, arrives as lexed.
- **Why:** the boundary already orders two cells of one rank by associativity, the right one first for `right`, so a read at discovery keeps the order the boundary would have had. A chain of choosers is one rule and no array rule. Thobias: "the first array parses then it parses the next array which consumes the i32 and emits a minted array. then the first array emits another minted array and that minted array has to read the elements of the outer list and construct each of the fields with the type the outer array holds which is array i32".
- **Ruled:** 25 September 2026, Thobias (the case); the general form Claude's, asked by Thobias to decide from DESIGN.
- **Seed:** 25 September 2026, in the driver's lazy read, no array-specific code.
- **Source:** DESIGN.md l.211

**Lazy tape reads**

### A `tape[k]` read past the lexed frontier lexes on demand, for every constructor
Every index a tape native takes (`tape[k]`, `tape[k] = …`, `tape.is_constructed[k]`, `tape.spelling[k]`, `tape.insert`, `tape.remove`, `tape.recenter`, and `tape[k]:type`, `tape[k]:name`, `tape[k].dyads`) lexes the driver's tape on to cell k first, through the one lazy read the built-in readers use. A cell arrives exactly as it would to `:` or application; a bracket as its built scope cell. A tape that is not the one the running constructor was handed (a `lex «…»` fragment, or any read at run time) is never lexed on.
- **Why:** Thobias: "its lazy lexing in tape so you you request something further down the line which isnt lexed it gets lexed automatically". The tape is lazy, and one rule then holds for every constructor.
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** 25 September 2026 (#137).
- **History:** from #61 (9 September) until 25 September, such a read by a Logos constructor was the checked error, because it re-entered the driver from inside a constructor; a Logos constructor at or above `(` could not lex past the frontier.
- **Source:** DESIGN.md l.211

### A read where the tape reaches no cell gives a `void` node, already constructed
Past the end of the source, past a boundary the tape does not cross for this constructor (the `,`, the enclosing closer, and the body bracket a return type, a condition or a range stops before), and left of the first cell alike: `tape[k]` is that one node, `tape[k]:type` is `void`, `tape.is_constructed[k]` is `true`, `tape.spelling[k]` is the empty text. A read that needs more than a cell has (`tape[k].dyads`, `tape[k]:name`) is the error it is on any cell without one. So `-> array i32 ( … )` takes no elements.
- **Why:** Thobias: "maybe tape should be able to return a void node to express there is nothing more here … so if you write tape[1] and you get void then the parse decides what to do and yes it should be the same as the void type. they do the same thing. but this void which is just emptiness after the string is already constructed instead so it stops things from parsing when at the end." "Nothing more here" is a value a parse tests like any other (`if tape[1]:type == void …`): no error handling and no second query for the common case; a `void` already built cannot be parsed on.
- **Ruled:** 25 September 2026, Thobias.
- **Open:** Claude's, open to Thobias: the return-type/condition/range stop (from the seed's sealed read); the boundaries and left edge counting as "nothing more here" (from the constructor's side each is the end of its reach); a write past the reach (`tape[k] = …`, `tape.is_constructed[k] = …`) stays the checked error, no cell to write; `insert`, `remove`, `recenter` keep the seed's behaviour; a `void` value reads as nothing, as a comment node and a `-> void` call do ("they do the same thing"), so a `void` a constructor writes into its own cell is a finished line that yields nothing and the loop needs no rule for it.
- **Rejected:** the queries `tape.last` and `tape.at_end` (first part of #147), not built.
- **Seed:** 25 September 2026 (#147). A tape read to its end point is sealed, so a later read past it gives `void`.
- **History:** after the lazy-read ruling, such a read was the checked error "this index is off the tape". #147's second part (a tape index failing as an error value under `T!`/`try`) has nothing left to do for reads; it stays staged after v0.1.0 for a write past the reach only.
- **Source:** DESIGN.md l.211

**Seed status**

### Seed tape shape (#60, #121)
A cell is the binding the trie resolved, a fresh dyad for an unknown spelling, or the node; `is_constructed` and the span are the tape's own facts. `parsing_tape` is a spelled type whose value holds the tape's handle. Its natives: `tape[k]`, `tape[k] = …`, `tape.is_constructed[k]`, `tape.insert(k, cells)`, `tape.remove(k)`, `tape.recenter(k)`, and `tape.spelling[k]` (#121). A Logos function reaches them through a `@parsing_tape` parameter. `tape.spelling[k]` hands back a string node built into the parser's store, the seed's form of a string value until strings are live: a string still has no storage to read, so a constructor may write the text into a cell, but nothing may yet run that cell.
- **Seed:** 9 September 2026 (#60); spelling 14 September 2026 (#121).
- **Source:** DESIGN.md l.211

### Seed stand-ins that remain
- A one-expression bracket is unwrapped to its expression, so the bracket a `(` landed is marked on the cell rather than read off the scope type.
- A member call passes its receiver as an address (the seed's form of a member reading the value's fields bare).
- A record-typed declared place is its layout's bytes, so `tape := parsing_tape ?` passes the tape by value (9 September 2026, #61); a record wider than the 8-byte container reaches a call only with #47.
- The tape natives run interpreted.
- A type body's constructor, written in Logos, runs for every appearance with the driver's own tape as argument and the parser's store attached (#61, 9 September 2026).
- The `parse_rank` expression runs at the definition and must be known there, since deferral to first use needs the deferral machinery.
- A Logos constructor at or above `(`'s `parse_rank` runs at discovery; its reads lex on demand (since 25 September 2026).
- `drop = fn …` is the checked error rather than a slot `drop x` would never run. The `fn` wrapper went 17 September; the slot moved into the type body with the mark (#132, 19 September 2026); an owner's teardown runs it since 25 September 2026 (*Explicit heap, and no implicit destruction*). A type's own `drop = (…)` on a bare line is the checked error.
- A literal appended into a call node is not committed to the callee's parameter type, the same divergence as the op slot (#69).
- `[` folds `t[k]` in its own constructor, a tape value having no constructor of its own yet.
- **History:** l.211 described a `type (…)` body with four slots filled by `=` and a `fields = (…)` block (spelled `instance` in the seed until #128) taking holes and bare names only, stored-once members there waiting. The fields block is gone since 25 September 2026 (l.203; seed #150).
- **Source:** DESIGN.md l.211

### A dyad shows two fields, `type` and `value`; everything else comes through the type
A dyad (a cell) has exactly two stored fields, `s.type` and `s.value`, and nothing more. Reads that decode a value are not on the dyad. They are ordinary `.` reads on the value itself, through the value's own type: an operator node's slots are the fields its type defines, so `(x + x).operands[0]` reads the first operand just as `p.x` reads a record field. An array's elements and a string's characters belong to the array and string types' own fields in the same way.
The fields exist only where `s` is a dyad. Everything shared lives on the type and is reached through it: `s.type.arity` and `.roles[i]` (an operator's slot count and slot names), `.parse_rank`, `.associativity`, `.parse`, `.drop`, and the layout reads `.fields`, `.size_bytes`, `.scope`.
There is no `.kind`: the type is the kind, and you classify a value by comparing its type against known identities.
A read that does not fit the node's type (`.text` of a number, an operand index past the arity) is a checked error. Answering `?` instead waits for the `?` identity.
- **Why:** `.` does one job: it reads fields the type defines, and every such field is about the value. A value's type is never one of its own fields, so a universal `.type` on every value would be `.` doing a second job.
- **Ruled:** August 2026, in discussion (corrected August 2026 after the seed briefly put the decoding reads on the view).
- **Rejected:** a `.kind` field, a second classification axis: redundant with the cell's self-description. The universal `.type` metaproperty on every value: retired.
- **Seed:** since August 2026 (#52 in part): `.operands[i]` is ordinary field access on any operand-kinded value; the type members (`.arity`, `.roles[i]`, `.parse_rank`, `.associativity`, `.parse`, `.drop`, `.fields`, `.size_bytes`, `.scope`) read the shared record as fields of the type value; every read folds at parse (comptime reflection, the regime a Logos-written constructor runs in); a mismatched read is the checked error.
- **Open:** in the seed: `.items` and `.text` (the array and string types' own fields), runtime-indexed reads, and runtime-callable method forms.
- **Source:** DESIGN.md l.213

### `a:type` reads the type of the value a name stands for
`x:type == i32`, `i32:type == type`, `a:type is number` in a premise, `tape[-1]:type` inside a constructor, `b:start.rhs:type` on a node reached by path. A value's other fields are reached with `.` on the name. Nothing reaches the cell as a whole. `:type` is the one `:` read that reaches the dyad level. The binding gate stands at `:type` (see *Sections, the arche, and effect identities*).
`a.type` is an ordinary field read on `a`'s value, present only if its type declares a `type` field.
- **Why:** Thobias: "i want to use a:type instead of a:dyad.type. its shorter and you dont really need to access a:value as you can access all its fields by just using .[name]". The type is the one fact about a value that `.` cannot give (it is never one of the value's own fields), so it takes the short spelling. The cell was only ever reached to get its two slots: one is now `:type`, the other is `.`.
- **Ruled:** 23 September 2026, Thobias.
- **Rejected:** `(a as dyad)`: the less consistent spelling.
- **Seed:** since 24 September 2026 (#139): `:dyad` and `:value` are checked errors; the binding's pointer to its dyad stays a layout field that `:` does not spell.
- **History:** August 2026: the dyad view was `(dyad a).type`, replacing the universal `.type` metaproperty. 7-8 September 2026: respelled `a:dyad.type` (`:dyad` = the view; `(dyad a)` as a second spelling dropped). In that period `a:type` was a checked error. 23 September 2026: `:dyad` and `:value` retired, `a:type` replaces `a:dyad.type`.
- **Source:** DESIGN.md l.213

### Writing a cell's fields is decided by gates
Writing goes through constructors and the tape ops, never setters. What may be written is the gate's decision (*Mutability and construction*). A constructed node with no binding has no gate. How other reads are gated is deferred to the gate section.
In v0.1.0 the view is ungated: every field readable and writable, until gates land.
Direction (recorded with the August ruling): gates over the view are the fine grain over the coarse section default: a value a gate makes unviewable as dyad, a `.type` readable but never writable, a `.value` sealed both ways (see *Sections, the arche, and effect identities*).
- **Ruled:** 31 August 2026 (read-only after construction); 2 September 2026 (v0.1.0 ungated, staged); 7 September 2026 (gates decide).
- **History:** 31 August 2026: a constructor could write `tape[0].type` and `tape[0].value` while its own cell was unconstructed, and "the flip to constructed seals both against the view". Superseded 7 September 2026 by the gate ruling of *Mutability and construction*.
- **Source:** DESIGN.md l.213

### Element access is `[…]`, application is `(…)`
An object storing many elements, whatever its storage or indexing, is a **collection**. Reading a stored element is always `c[k]`: an array by position, a map by key, a set by membership. Each collection kind picks only its index domain. So three surfaces, three jobs: `.` fetches a field, `[…]` fetches an element, `(…)` computes. A node's operands are the collection its type defines, `(x + x).operands[0]`; a type's role names are `.roles[i]`.
Model underneath (for the proof layer): a collection is a function from its index domain to its elements (shapes and positions), which also makes every strictly positive inductive type one.
- **Ruled:** August 2026, in discussion.
- **Source:** DESIGN.md l.213

### `x is c` is membership
`x is c` is true exactly when `x` is an element of collection `c`. It is a plain boolean operator with no proof machinery, like `==`. A premise says "a is a number" as `a:type is number`, `number` being the collection of numeric types the library defines (*The proof layer*). Inside an Englogos block `is` is that language's copula; a language is a closed section, so nothing collides.
- **Why:** with inheritance rejected and `==` on types being identity, "is a" had no spelling. Membership in a collection anyone can define gives that relation without a second classification axis between types.
- **Ruled:** 8 September 2026.
- **Source:** DESIGN.md l.213

### `A ⊆ B` is inclusion between types
`A ⊆ B` is true exactly when every value `A` can hold is also a value of `B` ("all possible values in LHS also in RHS"). It is its own identity beside `is`. Its rank is `==`'s, above `not`, so `not a:type ⊆ b:type` negates the inclusion; this rank is a stand-in, no rank being ruled.
- **Why:** an array's element read must say its index type cannot reach outside its size type, and neither `is` (one value's membership) nor `==` (identity) says that of two types. Since 25 September 2026 the read takes any integer index and a bounds check keeps it inside (*An index may be of any integer type*); `⊆` now sorts the index to `at` or `at_signed`. In the seed: array.logos checks that an index type fits the size type, and keeping that check beats deleting it.
- **Ruled:** 23 September 2026, Thobias (first ruled not for the seed); 25 September 2026, Thobias, "lets go for ⊆" (the seed carries it as a native).
- **Open:** Claude's, 25 September 2026, open to Thobias (with *Each element is built by the element type*): a type built by a Logos `parse` includes only itself, and no other type includes it. So `3 ⊆ array i32` is false, and `array array i32 [[1, 2], 3]` is "an element does not fit the element type" rather than this error. Reason: its values are the nodes its own `parse` builds (*A value of a type built by a Logos `parse` travels as a pointer*), which no other type's value is.
- **Seed:** since 25 September 2026 (#137): `⊆` between two types, folded at parse when both are known, run interpreted when one is read at run (a tape cell's `:type`). A type includes itself. Two integer types compare by value range: `u8 ⊆ u16` and `u8 ⊆ i16` true, `i8 ⊆ u64` false. Any other pair (float and integer, two floats, `bool`, a record, `type`, `dyad`) is a checked error, what those types hold not being settled here.
- **History:** array.logos first wrote `index:dyad.type ⊆ this.size:dyad.type` (old `:dyad` and `this` spellings).
- **Source:** DESIGN.md l.213

### `.size_bytes` is a `u64`
`i32.size_bytes` is `u64 4`, `(@u8).size_bytes` is `u64 8`.
- **Why:** it counts bytes, which are never negative. array.logos multiplies it by the array's `size`, a `u64`, so a byte count and an element count meet with no conversion. language_sketch.logos already spells a type's record `size_bytes -> { type -> u64 }`. Thobias: "u64 is fine".
- **Ruled:** 25 September 2026, Thobias.
- **Seed:** since 25 September 2026 (#137), for a record, a number type and a pointer type.
- **Source:** DESIGN.md l.213

### `:` reads a name's binding, `.` reads a thing's own fields
Every declared name has one **binding** (the trie entry). Its fields: `scope`, `start`, `end`, `gate`, `name`, `lex_rank`, plus a pointer to the dyad that is not a spelling. `a:scope` is where `a` was declared.
A binding is a value: a dyad of type `binding`. Its reading rule is "the dyad it names": read as a value it yields what that dyad yields, so a name can stand anywhere a value can. `:` reads a field of the binding itself, bypassing the reading rule; `.` reads through it. `a.x` reads `x` from a's value, `a:scope` reads `scope` from a's binding.
A use of a name in code (an operand of `+`, the right side of `:=`, an argument) stores the binding, never the dyad. The tape hands a constructor the binding for a resolved spelling (*The scope's constructor is the driver*); the constructor stores the cell as it stands; the interpreter reads through the reading rule at run time; compiled code bakes the address. So code walking the graph that reaches a named operand holds its binding and asks it `:scope`, `:gate`, `:type` directly, with no index and no binding stored in the cell (*Meta-navigation*).
An anonymous node is reached through a field that holds it: in `b := a + 1`, `b:start.rhs.type` reads the `+` node's type (`b:start` is the declaring `:=` node, since b's value is the number, not the code). A name gets you a binding, a binding gets you a dyad or a node, a node's fields get you more nodes, and only the first hop uses `:`. Exception: a constructed node reached by path answers `:scope`, `:start`, `:end`, `:gate` from the path (*Meta-navigation*, 8 September 2026).
A `:` read the binding has no field for is a checked error, exactly as a `.` read the type does not declare.
- **Why:** with the reverse index and the in-cell binding both rejected, a use pointing at the bare dyad would leave the binding unreachable by structure. A binding that is a value makes `:` a field read like any other, not a second mechanism. `:` and `.` are two operators because they read two levels (next rule).
- **Ruled:** 7 September 2026 (`:` reads the binding; `id_context` renamed `binding`, see *`mut` is a gate on the binding*); 8 September 2026 (a binding is a value, a use points at it); 14 September 2026 (`name`, `lex_rank` added).
- **Seed:** since 8 September 2026 (#70): every trie entry is a dyad of type `binding`, a use stores it, `:` reads its fields. A constructed node answers `:scope` with the scope open at the read and `:start`/`:end`/`:gate` null, the enclosing item being the segment still under construction.
- **History:** 7 September 2026: the binding held `dyad`, `scope`, `start`, `end`, `gate`; `a:dyad` was its cell. 23 September 2026: `:dyad` retired (see `a:type` above). The `b:start.rhs.type` example corrected 8 September 2026.
- **Source:** DESIGN.md l.213

### Two levels: the binding is the lex level, the dyad is the value level
Everything the lexer owns is tied to a name or to an appearance. Per name: the binding, `name`, `lex_rank`, `scope`, `start`, `end`, `gate`. Per appearance: the tape, `tape.spelling[k]`, `tape.is_constructed[k]`. These are read with `:` on a name, or with the tape's reads inside a constructor, and none is ever assigned (one exception, ruled 19 September 2026: a constructor writes its own cell's flag, `tape.is_constructed[0] = true`, when it has constructed something; see *Execution is function application*).
Everything else is the dyad level, `type` and `value`, read with `.` through the type. `:=` binds a name to the right side's dyad (a value bringing its type with it); `=` writes the value slot and keeps the type.
Parse behaviour is not a third level. It is content of a value: a type identity's value holds `parse_rank`, `associativity`, `parse` because its type, `type`, declares those places; a `5` holds none because `rational_number` declares none. That is why these three cross `:=` with no special rule, and `lex_rank` does not (*The constructor is a field*, 14 September 2026).
- **Why:** this is also why `:` and `.` are two operators: `:` reads the lex level, `.` the dyad level.
- **Ruled:** 14 September 2026.
- **Source:** DESIGN.md l.213

### `x:name` is the name's spelling, held by the binding
`x:name` is the spelling the trie holds for `x`: «x», or the pattern text for an identity declared `regex «…» := type (…)`. It is a string, readable wherever `:` reaches: a constructor holding `tape[-1]`, an error message, documentation tooling, a `#` comment interpolating a name, the proof layer printing a conjecture. `:name` on a constructed node reached by path (`b:start.rhs:name`) is a checked error, a node having no name.
- **Why:** the trie already keys on it, so the binding only gives it back; nothing is indexed after the fact, which separates this from the `uses` field declined 8 September (*Meta-navigation*). It is the binding's, not the dyad's, because `x := i32` gives i32's dyad a second name with a different spelling. It is not the tape's, because the pattern identity for numbers is one binding for every `5` in a program. So a name's spelling is the binding's fact, and an appearance's text is the tape's, `tape.spelling[k]` (*The scope's constructor is the driver*).
- **Ruled:** 14 September 2026.
- **Seed:** since 14 September 2026 (#120): the binding's sixth slot, a place of type `string` holding the name node. `x:name` reads it as the container it is; the display shows its text. A constructor reaches it for the binding a cell holds as `tape[k]:name` (always the identity's name, never the appearance's text, which is `tape.spelling[k]`).
- **Source:** DESIGN.md l.213

### Build and run are one pass that directs itself
Lexing, parsing and running interleave. A construct can `compile` and then `run` a function during the same pass, at once. There is no promotion scheduler in the seed: execution is directed explicitly where wanted; adaptive hot/cold promotion is a later layer over the same boundary. A `run` over a scope that compiles and runs on the spot is comptime metaprogramming in full: the whole language at parse time, working on real Logic Graph, not a macro sublanguage.
Comptime runs without the I/O capability, the same boundary that separates compile-time from run-time effects. Exception: `print` may write output at comptime (see the `print` rule below).
- **Why:** comptime stays reproducible because it has no I/O. Output reads nothing back, so `print` keeps the build reproducible.
- **Ruled:** base rule undated in the text; `print` exception 24 September 2026.
- **Source:** DESIGN.md l.215

### The pass runs only as far as it must, in order, and never twice
A scope nothing defers runs in parse order: its items run when the scope runs, after it has parsed, unless the parse itself needs a value (a box read where an identity is needed, a call for a type, a constructor written in Logos, a type's own body, an import). Then everything parsed before that point and not yet run runs first, in order. What ran stays in the graph as its result, `{type: ran, value: [expr, value]}`, so the scope's own run reads it and does not run it again. The item being parsed is not in that list, so order stays left to right.
A deferred scope (a function body, a loop body and its condition, a branch that may not run) is a body: nothing in it runs in the pass.
- **Why:** (1) a program that never needs a value during its parse runs exactly as written, left to right, and can be parsed whole before anything runs or compiles. (2) The same text means the same in a block, at top level, in a REPL line and in an imported file, since one mechanism serves all four. (3) Each such point is where running had to come before parsing, the one thing that keeps source from being compiled whole, so it is visible and countable, and absent from every function body (the parse refuses such reads there, so compiled code never contains one).
- **Ruled:** 13 September 2026.
- **Rejected, to stay rejected:** running a bracket's items the moment they parse (it would run the bracket in `bump() + ( x = 10, x )` before `bump`); a whole-program parse followed by a run (it cannot serve `x := a 5` over a box).
- **Source:** DESIGN.md l.215

### A type is a value; what the pass decides is the elaboration
A function may return `type`. A type value is a node address like any other: it can be passed to a function, held in a place, compared. What the pass decides is the work that needs the identity in hand: the layout a declaration claims, an `==` that folds, an `if` on a type that drops its untaken branch unparsed. That work gets no runtime answer.
**The line falls at `compile`, not at the language.** Under interpretation the graph can change as it runs, so a type reached at runtime can still be followed to its fields (*Metareflection from within the language*). Compiled code has already baked the decisions those fields would drive, so there a type value is an opaque eight bytes that passes and compares and nothing more. A place holding a type is an ordinary place; a place whose layout waits on a runtime type stays refused.
When a `-> type` call runs in the pass, the concrete type it produces stands in for the call: `a := metatype(0) ?` is the ordinary declaration over a computed type.
`==` and `!=` over type values compare by identity (sound because type identities are interned). An `if` whose condition is known at comptime folds at parse, its untaken branch dropped unparsed; this lets branches for other comptime types sit in one source.
- **Why:** the value travels, the elaboration waits. The seed already passed, held and compared types through an untyped parameter before any spelling admitted it.
- **Ruled:** July 2026 (types are comptime, "chosen over runtime type-values", `==` by identity, `if` folding); 12 September 2026 (value vs elaboration split, reading the July parenthetical as about elaboration, not forbidding a type-valued place).
- **Source:** DESIGN.md l.217

### A `type ?` place is a box, written any number of times
`a := type ?` declares a box. Write it as often as you like, read it as an ordinary value. Where the pass has already run the assignment that filled it, the box is read during the pass and what it holds is the declared logos: `a := type ?, a = i32, x := a 5` declares an `i32`. Where it has not (a body that runs later), the box holds a type at run time, and uses that need the identity now are the checked error.
- **Ruled:** 12 September 2026.
- **History:** before 12 September 2026 the first `a = i32` rebound the name to the type, making it define-once; a second assignment said a type is not an assignable place, because the name by then *was* the type.
- **Source:** DESIGN.md l.217

### `dyad ?` is the general box, `type ?` the narrow case
Both hold a node address. What the general one holds is asked the ordinary way, `a:type == type`.
- **Why the box needs a mark:** the type slot separates a value from a place only where the value is not itself an address. For `type` and `dyad` it is: a logos node standing as a value carries its identity as its value, so a definition and a box of one are both a `type` over an address. Pointers box every value instead (which is why `&x` allocates); types cannot, because the inline form is what lets a `-> type` function compile. So the mark is the cost of the ruling, and it covers both.
- **Ruled:** 12 September 2026, on the reading that the type slot should carry the distinction.
- **History:** written `a:dyad.type == type` until the 23 September 2026 respelling.
- **Source:** DESIGN.md l.217

### A tape cell checked to be a `type` passes as a `type`
`tape[k]` hands a constructor the cell. Where a `type` is wanted (a `type ?` parameter, or a `:=` whose box then holds a type), an unchecked cell is refused, since crossing types is explicit (*Numeric literals are uncommitted until context classifies them*). After a check the constructor already writes, the read is the type the cell holds:
- after `if tape[k]:type != type error «…»`, to the end of the scope holding the check;
- inside the branch of `if tape[k]:type == type`, and inside the `else` of the `!=` form.
Then `t := tape[1]` declares a `type` box and `get_mint(t)` passes, as identities/array.logos's chooser writes.
Limits: only a cell named by the tape and a written index narrows (never an index computed at run). The narrowing ends at the first tape edit after the check (`tape[j] = …`, `insert`, `remove`, `recenter`), since the cell at `k` may then be another. The read checks the cell again when it runs, so a tape edited between check and read in a way the pass cannot see (a loop) is the checked error, not a wrong address.
- **Why:** the check already says what the cell is, so no cast is needed. It is the binding's own reading rule: "read as a value it yields what that dyad yields" (*The dyad's read surface*).
- **Ruled:** 25 September 2026, Thobias, option (c): check `:type` first, then pass.
- **Seed:** since 25 September 2026 (#137).
- **History:** the text said "Only a check against `type` narrows: what a read narrowed to a number type would yield is not ruled"; superseded the same day by the next rule.
- **Source:** DESIGN.md l.217

### A tape read checked against a number type reads as that number
With `T` a number type, the read `tape[k]` yields the number the cell holds, as a `T`:
- after `if not tape[k]:type ⊆ u64 error «…»`, or after a raising `tape[k]:type != T`;
- inside the branch of `if tape[k]:type ⊆ T` or `== T`, and inside the `else` of the negative forms.
Scope rules as for the `type` check: the narrowing ends at the first tape edit; `not` before a check swaps which branch holds.
A line of a bracket cell, `tape[k].dyads[i]`, is checked and narrowed like a cell; `tape[k].dyads[i]:type` is the line's type (a name's declared type, an expression's result type, by *A placed call keeps its tape lines as operands*). array.logos writes `if tape[1].dyads[0]:type ⊆ this.size:type ( … ) else ( … )` (text's spelling; `this` is retired by l.207), a signed index going to `at_signed` as an `i64` (*An index may be of any integer type*).
A literal line has no committed type ("a concrete type beside a literal molds the literal to it"), so `⊆ T` answers for a literal by whether it molds into `T`: `[1]` passes `⊆ u64`, `[-1]` and `[1.5]` do not; the narrowed read is that number molded. A line that is a constant of `T`, `i32 7`, reads as its value.
A line that is an expression (a name included) is the checked error when the narrowed read runs in the parse, never evaluated there: the value exists only when it runs ("Only a use that needs the type during the parse … is the checked error"). Passed to a call a parse places, a narrowed line is the line itself, kept as graph for the call to run and read there as a `T` (*A placed call keeps its tape lines as operands*, 25 September 2026).
- **Why:** one check-then-narrow rule for types and numbers; the check already says what the value is, so no cast is needed.
- **Ruled:** 25 September 2026, Thobias, option (a) of the #137 question.
- **Open:** two readings, Claude's, 25 September 2026, open to Thobias: (1) a literal answers `⊆` by molding, the only reading under which his `⊆ u64` example passes a literal index. (2) A line index that is a name, the same name in check and read, names one line, so the mint's `fill` loop `for i in 0..this.size ( if not (elements.dyads[i]:type ⊆ this.element_type) error «…», (this.ptr + i)@ = elements.dyads[i] )` narrows (a bracket handed to a call is read as a tape cell's bracket is, *A bracket goes to the call whole*; the condition bracketed because the seed lets a type read through `.` take the cell after it), while the cell index stays literal. Safe because the read checks the line again when it runs.
- **Seed:** since 25 September 2026 (#137).
- **History:** before 25 September array.logos wrote `if not tape[1].dyads[0]:type ⊆ this.size:type error «…»` and passed the index to `this.at` as a `u64`; changed by *An index may be of any integer type*.
- **Source:** DESIGN.md l.217

### A block settles its boxes as the top level does
Reading a box during the pass is honest only if the answer does not depend on where the box sits. Lexing the box's cell runs what stands before it, once (*The pass runs only as far as it must*). Inside a deferred body (a function, a loop, a runtime branch), parse order is not run order, and a box used where an identity is needed is the checked error.
- **Why:** it used to depend: the top level ran each item as it parsed, while a `( )` block parsed its whole body first, so the same source meant two things.
- **Ruled:** 12 September 2026.
- **History:** 12 September 2026: the stores filling such a box were replayed at parse, in parse order (the right side a node address fixed at parse, so the run stored the same bits again). Replay superseded 13 September 2026 by *The pass runs only as far as it must*: nothing is replayed.
- **Source:** DESIGN.md l.217

### Inside a body, a `-> type` call may take an argument known only at run
A `-> type` call whose arguments are types or literals runs in the pass, and its type stands in for the call. One whose argument is known only when the body runs (a parameter, a local, a tape cell) stays a call, runs with the body, and returns a type value at run time, which may be compared, held in a `type` box, passed on and returned. Only a use that needs the type during the parse (`pick(n) ?`, `pick(n) 5`, a read of its fields) is the checked error, "known only when the program runs".
- **Why:** the value travels and the elaboration waits. Refusing the call refused the value with the elaboration, and array.logos's chooser calls `get_mint(t)` with a `t` its parse reads off the tape at run.
- **Ruled:** 25 September 2026, Thobias, "yes" (the #137 mint question). Narrows the July sentence that read as if every such call ran in the pass.
- **Seed:** since 25 September 2026 (#137).
- **Source:** DESIGN.md l.217

### A `type (…)` inside a body is built each time it runs
A `type (…)` written where parse order is not run order (function body, loop body, run-time branch) is not built once where written, where the body's names hold nothing yet. It is held as its lexed tape (the level-one deferral of *Deferral is authored*), and each time its node runs the parser builds it from those cells with that call's values live. So `mk := fn (t := type ?) -> type ( type ( fields = ( shared e := t ) ) )` gives a new type per call whose `e` is that call's `t`. The running program calling back into the parser is the one pass building on demand, not a second pass.
- **Why:** a chooser mints an anonymous `type (…)` for a parameter it has not seen and that is known only when its body runs; building at the definition would use names that hold nothing. The text is lexed once and only its cells are built again, so nothing is re-parsed.
- **Ruled:** 25 September 2026, Thobias, "yes" (same question).
- **Seed:** since 25 September 2026 (#137): a held `type (…)` builds a fresh type on every run, so one type per element type is array.logos's `array_mints`, not the seed's.
- **Source:** DESIGN.md l.217 (the example keeps the text's `fields = (…)` / `shared` spelling; see report)

### A generic type is written by juxtaposition: `array i32`
`array i32` is the type "array of i32": a specialized identity, interned, so `array i32 == array i32`. `array i32 (1, 2, 3)` is a value of it, `a := array i32 ?` a declaration. A generic type is a chooser like a generic fn (*Deferral is authored*): each parameter yields its own concrete identity, built once. The constructor decides.
- **Ruled:** 30 August 2026.
- **Rejected:** `array(i32)` (collides with conversion); `array[i32]` (`[]` is element access).
- **Source:** DESIGN.md l.217

### The chooser keeps its mints in a memo, and the driver constructs the mint
`array` is a constructor for another word with its own parse and value; `array` itself need not have a value. Its mints live in a memo that is not instance data (a chooser has no instances). Today the memo is `shared array_mints := hashmap type -> type`, the first line of `get_mint`, the function the chooser's `parse` calls to fetch or mint. The chooser looks the parameter up by `==`, mints an anonymous `type (…)` when absent and adds it, places the mint in its own cell and leaves `tape.is_constructed[0]` false. The driver then constructs the cell in its turn like any unconstructed cell: it wakes by the flag and the cell's `parse_rank`, never by a spelling, so an anonymous type needs none (*Execution is function application*).
`array i32 ?` is then the hole's ordinary reading of the type to its left. `t := array i32` leaves the mint standing as a type value (unlike `m := hashmap K -> V`, which declares an empty map, *The reflection boundary is callable-versus-data*, 25 September 2026).
The sketch is identities/array.logos.
- **Why:** Thobias (18 September): "array should actually just be a constructor for another word with its own parse and value while array doesn't necessarily have any value at all … there should rather be an array_mints to store all the mints of array". The memo sits inside `get_mint` because only `get_mint` reads or writes it, so it lives in the function that owns it, not as a top-level name any line of the file could touch.
- **Ruled:** 18 September 2026, Thobias; driver half 19 September 2026; memo placement 25 September 2026, Thobias (his own edit of array.logos, under the ruling that `shared` names a function's own place, *Two muts, and the storage partition*).
- **Rejected, to stay declined (18 September 2026):** `a:type is array`, a generic read as the collection of its mints, for "is it some array". Thobias: "isn't it enough for always?": the mint is interned, so `a:type == array i32` answers every concrete question, and generic code writes `array T` with a hole the chooser matches, so no use site needs the wider predicate.
- **History:** 18 September 2026: memo `array_mints` a plain top-level variable beside the chooser, read by its `parse` as an outer name (a top-level name read in a body is the shared place itself, *Memory and concurrency*; a call is a use of every outer name, #125); the chooser ran the placed cell's `parse` itself. 19 September 2026: the driver runs it by the flag. 18 September 2026: reinstating `shared` for a fields-block member proposed and withdrawn within the minute (the memo is not instance data); the withdrawal superseded 19 September 2026 (`shared` reinstated as the mark of placement, *Two muts, and the storage partition*). array_mints.logos folded into array.logos 19 September 2026. 25 September 2026: memo moved into `get_mint`.
- **Source:** DESIGN.md l.217

### Deferral is written by the author, and unfinished work is visible
Progress is driven by data, never scheduled: a node missing an input stays built and moves on the moment the input arrives. One rule at three levels: an unconstructed neighbour on the tape (the `is_constructed` flags), an unresolved type (the conserved op slot, which does not lower until resolved), an unknown runtime value (control flow arriving).
What a constructor cannot decide statically it **returns as residual code** (for example the collection's bounds check, emitted as a scope that runs when index and size exist). So every constructor is a partial evaluator, and "later" is always ordinary graph you can reflect on: unfinished is a visible `?`, postponed is a visible scope. No hidden thunk, phase or re-parse: the scope is the thunk and forcing it is running it (the laziness *Execution is function application* makes the default).
Tolerance for the unresolved is **written per constructor, never ambient**. A constructor either accepts a `?` in a slot and holds, or refuses it as a checked error (a strict signature). Fail-closed wherever nobody opted in.
- **Ruled:** August 2026, in discussion.
- **Source:** DESIGN.md l.219

### A generic function body is lexed once and built once per set of field types
Accepting `?` and holding is what a generic function is: `fn`'s constructor accepts parameter types not yet known. The body is held as its lexed tape (the level-one deferral, what `lex «…»` yields) and constructed once per field-type set when a node supplies the types; its op slots resolve at that construction as anywhere else. Example: `^`'s `run = (…)` is lexed at the definition and constructed when `x ^ 3` is built, once for i32 operands, kept on the type for every later `^` over i32. The interpreter and the compiler read the same constructed body.
A generic `fn` is a chooser, exactly as `+` is: the concrete function for each argument-type combination is its own identity, built once from the written body with op slots resolved, then reused. A call site with known types resolves to that identity by the same lowering rules that fill `+`'s op slot, and stores it in the dyad as `+`'s op slot stores `add_i32`. The written generic body never runs itself.
- **Why:** the level-one deferral needs no new mechanism (cells and flags exist). Filling op slots later would give every operator a slot to fill (#69), and reading text again is the re-parse the deferral rule forbids.
- **Ruled:** 30 August 2026 (generic fn is a chooser); 8 September 2026 (same lowering rules as `+`); 20 September 2026, Thobias (lexed once, built per field-type set).
- **Seed:** #126; since #133 slice 8: the body held as lexed cells and constructed per set; a bare literal in an untyped field gives the `rational_number` set, whose body runs over rational places and operators, interpreted only; a node whose value fields are all literals folds at construction to its literal.
- **History:** August 2026: the body was "parsed once with unresolved op slots that a call's concrete types fill, walkable meanwhile through `run`'s null-`bcode` path". The text's example was spelled `shared run = (…)` (before *Fields block collapse*, 25 September 2026, which gives `^.run`).
- **Source:** DESIGN.md l.219

### The command line is Logos source; the binary stays out of the way
Everything after `logos` is one line of Logos code, run by the one pass: `logos import ./main.logos, main(«hello»)` imports a file and calls a function it exposes; the `,` is the ordinary separator that stops `import` from consuming more. There is no file position and no argument position: running a program is `logos import ./file.logos`. There is no `main` entry point: the top level is the program.
A program's command-line interface is its `pub` names. Arguments are not a separate channel or mini-language. A run is a pure function of the command-line source plus the files it imports (comptime reads no input).
Bare `logos` is the REPL over the same one pass; the command line is a one-shot REPL line. A failed line rolls its declarations back out of the name index, leaving no trace.
The binary has no subcommands and no compile flags, ever: it joins its arguments into source and runs them. What compiles, links and builds is decided inside Logos source. `--help` is the only flag.
- **Ruled:** July 2026 (binary with no flags); August 2026, in discussion (command-line-as-source, `import`, pub-only exposure).
- **Seed:** converged August 2026 (#58): argv-as-source with file-path mode removed, `import` with pub-only exposure, once-per-run DAG loading, the fresh-view section boundary (command line and REPL each run as their own section), file-relative path resolution.
- **History:** `logos file.logos` superseded August 2026: ambiguous against the since-retired `.logos` metaproperty (`file.logos` read as "the logos of `file`").
- **Source:** DESIGN.md l.221

### `import` is the one identity that loads a file
Its constructor consumes the path token (the same licensed token consumption as `#` and the planned header reader). The imported file runs top to bottom. An importer, command line or Logos file alike, reaches only the imported file's `pub` names: the ordinary visibility rule, with no import-specific exception.
- **Ruled:** August 2026, in discussion.
- **CONFLICT:** l.221: "(the target; v0.1.0 has no gates and an importer sees every top-level name, ruled 4 September 2026, see *Feasibility*)" vs l.375: "(Superseded 20 September 2026, Thobias: `pub` stays and `mut` lands in the seed before v0.1.0, because the gate machinery is best tested early and `pub` already carries imports; #64 closed without the change.)". l.221 still states the 4 September staging as live.
- **Source:** DESIGN.md l.221

### `print «…»` is the output word
`print` reads the `«…»` to its right, as `#` and `lex` read theirs, and prints its text, `{…}` interpolating a value: `print «answer {double(sum)}»`. `print` itself runs each `{…}` and turns its value into text exactly as the tail-value echo shows it (`true`, `5.5`, `42`), so interpolation does not wait on live strings. The expression is parsed when the `print` is read, in the scope the `print` stands in, so it sees the names written there. `\{` and `\}` are braces as text; a `{` without its `}`, or a `}` without its `{`, is an error. `{…}` outside `print` and `error` still waits on live strings.
A `print` may run at comptime (for example inside a constructor, while the program is parsed) and is not an error.
The trailing value of a program prints: the seed's stand-in until `print` lands with I/O.
- **Why:** the website's examples used the spelling since 22 September with nothing in the spec behind it; one recorded word keeps the site and the spec one thing. The echo already shows every value the seed has, so one way of showing a value serves both. A backslash before a brace is the spelling readers know from other languages. Comptime `print`: printing only writes, so a run stays a pure function of its source, and a constructor can show what it is doing while it parses.
- **Ruled:** 23 September 2026 (`print`, `{…}` details); 24 September 2026 (`print` at comptime).
- **Seed:** since 24 September 2026 (#140): each run writes the quote and a newline and yields unit, a statement the echo rule keeps silent.
- **Source:** DESIGN.md l.221, l.215

### Importing drops the text there, wrapped in its own scope
The imported file runs where the `import` stands, top to bottom, in the same one pass, but inside its own scope; the wrapper is the whole module system. Upward, only `pub` escapes. Downward, the wrapper is a section boundary (*Sections, the arche, and effect identities*): the imported scope's trie resolves ambient names and its own imports only, never the import site's surroundings. Effect identities reach it only as references handed to its `pub` functions at call time, so an imported top level is pure computation, and a dependency can do exactly what its callers pass it the means to do.
Paths: the path token ends at whitespace or `,`; a path those cannot spell is a quoted `«…»` string. A relative path resolves against the importing file's folder, and against the working directory only when the importer is the command line.
A file loads **once per run**; all importers share the one loaded scope and its identities, so two importers see the same types, never two incompatible copies.
Two imports exposing the same `pub` spelling into one scope are the ordinary no-shadowing error at the second import. Library idiom: one `pub` namespace identity per file, `pub math := type (…)`, read `math.sin`.
The import graph must be a **DAG**: a cycle is a checked error, never a resolution order.
An effectful program's invocation names its authority, `logos import ./app.logos, main(fs)`; the command line is the arche, so the shell line is the grant, visible in history. This shape is embraced on purpose.
- **Why:** the textual intuition is the true one. A file means the same thing wherever it is imported, which also lets the durable store cache it as one artifact. Once per run follows: the wrapper cuts off the import site, so a file's meaning cannot depend on where it is imported.
- **Ruled:** August 2026, in discussion (wrapper, once per run); 30 August 2026 (same `pub` spelling = no-shadowing error).
- **Rejected, to stay rejected:** bare textual splicing (`#include`): its leaked private names, double-inclusion collisions and import-site-dependent meaning are each what the wrapper prevents.
- **Source:** DESIGN.md l.223

### Two graphs, one source of truth
The **source graph** is file-backed, persistent, and the program's identity; a build is a pure function of it. The **runtime graph** is made during execution, short-lived, and owned like other runtime data: arena-allocated (arenas already carry the cyclic structures the graph needs), same lifetimes, no garbage collector.
Runtime structure is reflected on like source: by walking the structure, found by reachability from the use sites that reference it. There is no value-level or time-travel reflection: a value to inspect is stored in a variable, as in any language; rebuilding past states is the debugger's job over DAP.
Reproducibility needs no separate levels: a build reads only the files, and comptime runs without I/O, so the artifact is a pure function of the source. Cleanup needs none either: derived and runtime structure share the lifetimes of the scopes that make them, so when a scope ends its generated graph is freed like any data. That is *reclamation*, the effect-free return of memory, the one thing that stays implicit because it runs no teardown; anything with real teardown carries constructor-inserted `defer` structure (*Memory and concurrency*). When the program ends, everything generated is gone.
The runtime may change the source, but only on explicit command, a **promotion**: its dependencies are closed over; it must be free of runtime-only inputs (or have them reified as constants); it is copied out of its arena, unparsed to a source diff, re-checked, and committed under gate. Source is never silently rewritten. Unparsing any subgraph back to text is a presentation and serialization tool, not the reflection mechanism.
- **Source:** DESIGN.md l.225

### A scope may have several versions
A scope may have more than one implementation: the canonical one in the source, plus any number of derived alternatives (an optimized rewrite, a specialization for known arguments, a different backend) proven equivalent and chosen by cost. They are the structural form of the rewriting engine's equality classes: registered at the scope's entry, picked by a cost function. No global level numbering or edge classification governs them, because none is needed. A running program may attach optimized variants to a function's live form and drop them when the enclosing scope ends; the source file is untouched unless a variant is explicitly promoted. Cleanup is bounded by lifetimes; reproducibility is bounded by the files being canonical.
Not related to the proof layer's *universe levels* (the Lean-style hierarchy against self-referential paradox): same word, different mechanisms.
- **Source:** DESIGN.md l.227

### Backends are identities, and lowering is keyed by them
A backend is an ordinary identity carrying its own mapping from node kinds to emission code. Its rules live with it, so adding a backend is adding an identity and its rules, never a core edit. Many backends coexist: a scope's compiled forms are versioned-scope alternatives, picked by cost. Nothing per identity may hard-code one backend; lowering is per backend, per identity.
`run` stays backend-neutral: a compiled form is an address plus a declared convention, the call is the same whoever made it. Code from different backends works together through shared conventions, as foreign code does; co-compilation erasure only inside one backend's region. A backend declares a *gate* predicate over the scopes it accepts; what it rejects stays on other versions or stays interpreted, fail-closed.
Non-CPU backends (hardware synthesis) fit with two honest stretches, deferred: their artifact is not an `@exec` (the callable is a CPU-side driver stub, the artifact riding the version as data), and their call semantics want asynchrony, which is the effect system's surface, not the ABI's.
- **Why:** one lowering slot per identity would repeat the mistake of one mutable code field on `+`.
- **Ruled:** July 2026, in discussion.
- **Source:** DESIGN.md l.229

### Concurrent access: shared reads, disjoint or exclusive writes
Many threads may read shared graph structure at once with ordinary shared borrows (`&`): the standard library, definitions, any subgraph. Writes are disjoint (the `parallel for` pattern the borrow checker already knows) or exclusive (`&mut`); concurrent mutation of one dyad is rejected at compile time. Arena ownership governs lifetime. The one truly shared, mutable structure is the identity interner (allocating or looking up an identity from many threads must be synchronized), plus any equality graph that parallel rewriting builds.
- **Why:** per-node reference counts are avoided on purpose: they leak on the graph's cycles and track liveness, not the access exclusion threads need.
- **Source:** DESIGN.md l.231

## Identity recognition

How the graph answers "what is this?" for parsing, reflection and rewrite matching, with one engine. Target design; the bootstrap ships the trivial version (last rule of this section).
- **Source:** DESIGN.md l.235

### How an operation is defined for its operand types is OPEN
`x = 9`, `x = [4, 5]`, `h == j`, `a + b` share one question: which code decides what the operation does for these operand types.
- **Seed:** hard-codes it for core types. `=` is handed to the `parse` of the type of the place on its left (the l.189 sentence), which stands as seed practice but is in question. A square-bracket literal goes into an array's place in Rust, like number literals. Seed and spec issue #152.
- **Open:** to Thobias, nothing concluded (26 September 2026). His leaning, not a ruling: "i dont think the mint array should consume = but rather = should consume array mint … the operations them selves needs to be overloaded somehow instead". Operators would be defined in this section's identification system (not built): an operator can stand empty, as `->` does, and the system recognises *lhs op rhs*, finds both types and the operator, then decides what to do.
- **Source:** DESIGN.md l.237

### One engine serves parsing, reflection, and rewriting
Pointed at a node, the recognizer returns every identity that matches there: the operator a token sequence means, the construct a subtree forms, the rewrite rule whose left side fits. It is tree pattern matching (regular expressions over trees), extended with e-matching (over equality classes, not just syntax). Grouping, reflective queries and rewrite matching are clients of it.
- Recognizing tokens from source *text* is a separate layer: a string-keyed trie over names. Name and structure are two roads to the same identity.
- Matching is yes/no and exhaustive; the default query returns *every* match. No fuzzy or probabilistic matching in the core.
- Where one answer is needed (which parse wins), a fixed rank/cost order picks it: an order, never a statistic.
- **Source:** DESIGN.md l.239

### The scope stack is the graph's own spine
Navigation walks the Logic Graph itself. The parse-time scope stack is not a separate structure: it is the chain of enclosing scopes up to the root, read off the graph. Entering a scope goes down, leaving goes up, and name resolution is the walk up the spine, sped up by an O(1) open-scope set where the batch pass does not already carry the environment.
- **Ruled:** architecture reached in discussion, July 2026.
- **Source:** DESIGN.md l.241

### A scope node stores only its enclosing scope, spelled `back`
One link: the parent scope, null at the arche. A node that owns scopes (`fn`: parameter scope and body) is found from the node by comparing its scopes' addresses with the scope at hand; no scope stores its owner. The open-scope set is a cache over the link (the seed's `ScopeStack.open` is to become it). Chain up: `here.scope.back.back`, null past the arche. identities/scope.logos declares `back := @scope ?` beside `dyads`.
- **Why one link:** a second link to the owner was considered and declined the same day: the owner is already found from the node.
- **Why `back`:** on a scope, `.scope` read as the scope's own scope; the path word `back` was retired, so the word was free. `here.scope`, `caller.scope`, `x:scope` keep `scope`: "the scope of" a spot, call or name reads right.
- **Ruled:** link 15 September 2026 (#123). Spelling 24 September 2026, Thobias (he asked whether `s.scope` should be `back`).
- **Seed:** since #123 every scope is minted `[exprs, op, parent]`; `ScopeStack.open` is the cache. `.scope` on any `@dyad` value reads the link when it runs (`.back` since 24 September 2026), null at the arche, checked error past it. Addresses compare with `==`.
- **History:** link spelled `.scope` (`here.scope.scope`) until 24 September 2026.
- **Source:** DESIGN.md l.241

### `here` is where a line is written; `caller` is where it was called from
`here.scope` is the scope a line's spot is in. `caller` is the node a function was called from; for a constructor, the appearance of its identity (where it is implicitly called). Inside a constructor `caller.scope` is the use site (the tape's scope) and `here.scope` the definition site: the distinction macro hygiene needs, and the "where it is called from" gates read (*Sections, the arche, and effect identities*). Both are ordinary arche identities, like `lex`.
- `here` folds at parse like every reflective read, so it stands in compiled code.
- `caller` in an ordinary function is per-call, so a body reading it is elaborated per call node (as a gate is decided when the access is elaborated). For a constructor, `caller` is its tape.
- Both work inside a constructor. **Why:** every enclosing scope exists before a constructor runs, so walking up is over settled structure; only the frontier is unfinished, and the tape reads that.
- **Ruled:** 15 September 2026 (#123).
- **Seed:** `here`, `caller` are identities. `here.scope` folds to the scope open at the appearance, as `x:scope` does. `caller.scope` answers the pass's position while a constructor runs, checked error elsewhere: stand-in for the per-call read. Not carried: bare `caller` as a value; the section-boundary stop (waits for gates); a one-item group, which the seed elides, so `here` inside one names a scope no path reaches.
- **History:** `this` joined them 17 September 2026 (a name the slot's type declared, from 18 September); `this` is gone since 26 September 2026 (l.207).
- **Source:** DESIGN.md l.241

### A walk down stops at a section boundary; a function's scopes count as one
Walking *up* is free (ancestors are what resolution already offers). Walking *down* into a scope you do not stand in stops where a section begins: a function's parameter scope and body, an imported file, a language block. So an import never reaches the importer via the arche, and a function body is read only from inside. A `pub` name inside still answers, by its own gate. `if`, `while`, `type` and group bodies are rooms of the same house, open to the walk.
- **Why:** a section is already the region defined by which names reach it, so the walk honors the name index's boundary. A function breaks the pure scope tree (a function with a scope, not a nested region). The walkable spine also retires the old worry of *`mut` is a gate on the binding* that "almost nothing knows where" an anonymous node is.
- **Rejected:** a gate on every scope or every node: too much (same day).
- **Ruled:** 15 September 2026 (#123). **Seed:** not carried; waits for gates.
- **Source:** DESIGN.md l.241

### The parsing tape is the other axis
The tape is a scope's working frontier (not-yet-final dyads, pending tokens) that constructors consume into graph; the spine is the settled structure behind it. A matched bracket joins them: closing a scope reduces its tape frontier into one dyad on the spine. The walk a `parse` uses to reach its operands is the walk reflection and rewriting use over built structure.
- **Ruled:** July 2026 discussion; the meta-access syntax, left open then, closed 7 September 2026.
- **Source:** DESIGN.md l.241

### Reflection is over structure, never over running values
Every path into the graph is written in code, so known before anything runs. At runtime there are no dyads in flight, only values read through settled types. So a compiled artifact only reads frozen structure, and the image failure mode stays designed out. No reverse index, no binding stored in the cell.
- **Rejected:** a per-binding list of a name's users (8 September 2026, see *Identification is incremental*).
- **Ruled:** 7 September 2026.
- **Source:** DESIGN.md l.241

### A constructed node answers `:` from the path it was reached by
Reached as `f:dyad.value.body[0].rhs`, a node's `:scope` is the innermost enclosing scope on the path, `:start`/`:end` the enclosing item, `:gate` the named bindings along the path composed by the path rule of *`mut` is a gate on the binding*, and `:dyad` the node itself. Four facts, zero storage, because every path starts at a name. The pass walks the path; nobody spells the walk.
- **Why:** the path already fixes the facts. Storing them would cost 40 bytes per application and literal; an address-keyed index would serve a pointer-without-a-path, which never exists.
- **Ruled:** 8 September 2026.
- **CONFLICT:** l.241: "a `:` field no binding has, `a:type`, stays the checked error of *The dyad's read surface*"; l.189: "`:dyad` is retired and `a:type` is the read" (23 September 2026). The l.241 example also still uses `:dyad`.
- **Source:** DESIGN.md l.241

### The meta-access surface is `:` and `.`
`:` for the binding, `.` for a thing's own fields. Up the tree: the scope link `back`. Up the path: the path's own earlier step.
- **Why the path word `back` went:** Thobias, 24 September 2026: "i think back is just unintuitive. you can access all things with back just with .scope instead right? also having back is just extra things to store and manage". It stored nothing, but gave a reader nothing: its answer was always an earlier step the reader had written.
- **Open:** navigation words a two-node conjecture needs (*The proof layer*).
- **History:** 7 September 2026: `:`, `.`, and a `back` step walking up the written path one `.`/`:` at a time; retired 24 September 2026.
- **Source:** DESIGN.md l.241

### All recognizers compile into one shared, layered structure
Run over a node, the merged graph yields every matching identity, sharing sub-tests. Layers by cost: a fast *skeleton* (navigations; tests on identity, type, arity, existence), merged, minimized, indexed; repetition and nesting as cycles and a stack; arbitrary computation only in *guards* at the edge, on candidates the skeleton narrowed. Skeleton = speed, guards = expressiveness. Fast paths: forward dispatch, fail fast, no backtracking. Comparing two nodes is a guard, not a dispatch key; a relational or negative condition (operands differ) cannot prune the skeleton and rides as a leftover filter or an extra index, like a non-sargable database predicate.
- **Source:** DESIGN.md l.243

### The merged structure is ordinary Logic Graph code, extensible without recompiling
Not a trie or automaton walked by a special machine: ordinary graph code run over the start node, interpreted cold and compiled hot. Each identity's recognition code is first minimized and canonicalized by the ordinary rewriting engine under a compute-cost function, so recognizers branch alike and equivalent ones share paths. Adding an identity is a local, hash-consed merge, not a rebuild; the graph may drift from minimal and is re-minimized in periodic recompaction. Convergence is guaranteed on the decidable skeleton, best-effort above (program equivalence is undecidable; only identical guards merge).
- **Source:** DESIGN.md l.245

### Admitting a recognizer is a proof obligation; identification is proof
A recognizer is admitted at a recorded *level*: proven safe (terminates, deterministic, no effects) or, during bootstrap, not-yet-refuted. A use site demands the level it needs. Refuted, or resisting checking where proof is demanded, means rejected; nothing below not-yet-refuted, no escape past a use site's demand. "X is identity I" is the proposition "X has I's defining properties": a trivial pattern is a trivial proof, a rich property (totality, type-correctness, a non-conflicting borrow) a real one. Identification, type-, borrow- and termination-checking are one activity at different strengths, run on ordinary code, on extensions, and on the system itself, bottoming out in the small hand-trusted seed.
- **Source:** DESIGN.md l.247

### A type with a canonical form ships a normalizer; equality there is identification
Where a theory has a normal form (ring expressions → one polynomial; a rational literal → one reduced fraction, the seed's comptime folding being the first case), the type carries a **normalizer**: a recognizer mapping every expression of the type to its one canonical node. Equal means same node, and hash-consing makes that one address: `a + a == 2 * a` is identified, not searched (normalize both, compare one pointer).
- It is the guard tier doing what the skeleton cannot (arbitrary computation; "both operands are one `a`" is a comparison the skeleton cannot dispatch on).
- Admitted at level *proven*: terminates and preserves equality under the type's world, one truth rule for all `x`: `conjecture ( normalize(x) == x -> true ) where ( x:type == T )`, proven once, derivation checked by the trusted core. Or, in bootstrap, *not-yet-refuted*.
- Proven: "same node" is one accepted step citable in a derivation; one normalization replaces a saturation search. Not-yet-refuted: a plausibility ranked by the coherentist layer, never a theorem; a use site demanding proof refuses it.
- A proven normalizer emits no chain per use; any normalizer may emit one on demand (its own rewrites).
- It belongs to the type, not to a tactic inside a derivation (Lean's `ring` is the same computation placed elsewhere): the type defines how its value is read, and equality is a reading.
- No canonical form (most maths above algebra: irreducibility of a representation, modularity of a curve): equality stays *proven equal by the rules applied so far*, bounded saturation is the only path, no index changes that, the budget stands.
- **Why:** here the recognizer's near-linear lookup meets the proof layer. Equality costs one traversal, not a search, and the restated helper lemmas that drown the largest machine-written proofs (one normalization each) become free. Same shape as everywhere: a fast decidable canonical core, search only past it.
- **Ruled:** in discussion, 5 September 2026.
- **History:** conjecture respelled 7 September 2026; `where` read `x:dyad.type == T` until 23 September 2026.
- **Source:** DESIGN.md l.249

### Queries specialize the structure
A directed query ("is this matrix-addition?") intersects the question with the saved recognizers (automaton product, or generally partial evaluation baking in the query's constants), giving a specialized recognizer; computing it is itself the identifier's job. First pass over fresh tokens = discovery (all-match, builds structure); directed queries = interrogation of existing structure.
- **Source:** DESIGN.md l.251

### Identification is incremental
Durable state: source text, the resident name index (trie of bindings), the graph. No persistent token array; the tape is throwaway, rebuilt from source. Reader–writer rule: each cached identity reads the source span and rules it consulted; a write invalidates exactly its readers. Two non-local edits: redefining an *identity* ripples to its uses, found via per-subtree *use-sets* (which identities occur below each node, part of the bottom-up state); a structural edit (unclosed delimiter) re-lexes forward until the stream resyncs. The seed and runtime self-modification take the coarse path: the graph is mutated in place, and editing source *text* re-parses it (rare: programs rewrite their *graph*, not their text).
- **Source:** DESIGN.md l.253

### Re-parse runs from the changed step
A use before its declaration falls outside every range, so nothing elaborated before the edited item read the name. The coarse path re-elaborates the edited item and every later item in body order, and each importer from its `import` step on. Nothing before is touched.
- **Why:** a declaration read at comptime decides what is parsed after it (which `if` branch exists, what a `-> type` function returns, which names exist). Effects are transitive in elaboration order and are uses of what the name decided, not of the name. "From the step on" is the smallest surely complete set.
- **Ruled:** 8 September 2026.
- **Source:** DESIGN.md l.253

### The reader–writer rule is transitive, and resolution is a read
A re-run reader whose output changed is a write that invalidates its readers. A resolution read the trie as it stood, including the *absence* of an inner declaration a comptime branch could have made. The fine path is sound only counting both, so it stays target architecture until measurement asks for it.
- **Rejected, to stay declined:** a per-binding list of the declarations using a name (for every name, writable names only, or split by slot read). **Why:** it answers "who read the name" when the question is "what did the name decide", which no name-keyed list can hold; primordial names (`(`, `:=`, `+`, `i32`) would carry the largest lists for the least information (against *Storage is proportional to deviations*); the coarse path answers with nothing stored.
- **Ruled:** 8 September 2026.
- **Source:** DESIGN.md l.253

### This is the target architecture, not the seed
The bootstrap runs recognizers directly, unmerged, guards interpreted, and grows toward the merged minimized graph, e-matching and specialization only as measurement on real graphs warrants, never before. The recurring shape: a fast decidable canonical core, escape-hatch tiers, deterministic disambiguation, incremental compilation with periodic recompaction; the same as the borrow checker, compiler and rewriting engine, so one set of mechanisms serves all.
- **Source:** DESIGN.md l.255

## Identities, names, and metadata

Node store, optional name index, and how lifetimes and permissions attach without bloating nodes. Not yet checked against a real corpus; the bootstrap may use trivial versions and tighten later.
- **Source:** DESIGN.md l.259

### The store is keyed by address
An identity is an interned handle in a backing store with stable addresses (array, arena, allocator); its *address is its id*. Nodes are fixed two-pointer (16-byte) cells, so the address is an index: no key, no hash.
- A content → id hash is a *separate, optional* index, only where **sharing** is wanted: interned canonical identities (one `i32` everywhere) and the e-graph. Source nodes are not deduped: each `a + a` is its own node.
- Ids are *per-run*, never persisted. The durable form is source text (files canonical; promotion serializes by unparsing); the graph is rebuilt at load, with throwaway content-keyed caches where startup cost warrants.
- Anything that survives a session (caches, compiled artifacts, certificates) keys by content or structure, never by id.
- **Source:** DESIGN.md l.261

### Names are an optional index, not the management mechanism
Every identity is reached by its handle and by walking from its uses; the handle is what you compare, store and refer to. A name is a second index (string → handle) for what source refers to by word: operators, types, declarations, keywords. Most identities are anonymous subexpressions with no name.
- **Why:** naming all would generate millions of meaningless names: bloat.
- **Source:** DESIGN.md l.263

### The name index maps a spelling to a list of bindings
A **binding** pairs an identity with its declaring scope and holds its own spelling and `lex_rank` (`x:name`, `x:lex_rank`): the binding is the lex level, every fact tied to the name. A binding is a dyad of type `binding`, which reflection reads and a use points at.
- **Why `binding`:** Thobias, 24 September 2026, wanted a word more intuitive than `record`, "the opposite of anonymous": "i think binding is best". It ties a spelling to a dyad, in a scope, for a range, under gates (all the entry holds), and every language uses the word.
- **Rejected:** `name` (text read as `x:spelling`) and `naming` (24 September 2026).
- **Seed:** `record`/`Record` and the README followed as `binding`/`Binding` 24 September 2026 (#138).
- **History:** `id_context` → `record` 7 September 2026 (seed `IdContext` → `Record` 8 September, #67) → `binding` 24 September. `record` remains only for: an operand record, a record type, a type's own metadata head, the archival sense. Spelling and `lex_rank` on it since 14 September; a use points at it since 8 September 2026.
- **Source:** DESIGN.md l.265

### Resolution keeps the one open, live binding; no shadowing
A use keeps candidates whose scope is open (ancestor on the scope stack) *and* whose range covers the point. A name may not be redeclared while another declaration of it is live (one check at declaration time). So one survivor, or none: an out-of-scope use, which still lexes as the name and fails resolution with a precise error, not an unknown-token one.
- Two survivors is impossible, so it is a cheap internal check: two entries for one spelling never overlap on one scope stack; two survivors = corrupt index.
- A use written before its declaration is outside every range; later resolution from any context stays exact (the point decides, not the scope alone).
- A call is a use, at the call's point, of every outer name the callee's body reads (15 September 2026, *`own` and `drop` are static*).
- The walk runs over the scope spine, with an O(1) open-scope set during elaboration. The binding list lives for the whole run; entries go only when their declaration is deleted from source.
- The `drop` case of #130 dissolved 19 September 2026: slot names are the core words, one candidate (*The constructor is a field*).
- **Source:** DESIGN.md l.265

### Members are dot-only outside their scope; strictness has no exceptions
Outside its declaring scope a member is reached only by `.`, which probes exactly that scope, never as a bare name; so a field's no-shadowing check runs only against siblings. A type body's bare lines are live within the body and obey the rule like any block's. No exceptions: parameters, locals, loop variables, members alike.
- **Why it stays usable:** keep the ambient set to primordial names and ship the stdlib behind namespaces.
- **Ruled:** 30 August 2026.
- **Source:** DESIGN.md l.265

### A binding is live while its scope is open and it is not dead; it carries a range
`own` or `drop` make a name dead. A binding carries a **range** in its scope's body order: from the declaring node to the **item of that body** holding the killing `own`/`drop` (the line itself at the same level; the whole `if` when inside its body), else to the scope's end. While that item is being parsed the slot holds the `own`/`drop` node, so a later read in the same line already fails.
- Reflection reads the graph, not the index: the declare and killing nodes are body items and a teardown node stores the place it empties, so birth and death are a walk over the body; the index's two pointers are the derived fast path. Deleting a killing node resets the end to null, as deleting a declaration removes the entry.
- **Ruled:** 3 September 2026 (containing-item choice the same day).
- **Source:** DESIGN.md l.265

### The binding carries the name's gates; a partial move adds a sub-range
Gates sit beside the range: one lookup answers open, live and permitted (*`mut` is a gate on the binding*). `own p.f` adds a **sub-range**: it ends path `p.f` at that line (created only then) and marks `p` dead as a whole, sibling paths live (*Memory and concurrency*, 5 September 2026). Consulted at every elaboration (parse, graph mutation, reflection), never by running code.
- **History:** gates ruled 5 September 2026 as a separate identity binding; merged into the one binding per name 7 September 2026.
- **Source:** DESIGN.md l.265

### A dead name takes nothing; only `:=` may follow
The dead mark is static, set by the parser at the `own`/`drop` (three rules of *Memory and concurrency*). A read, write or pass after it is a parse-time error. Only `:=` may follow, redeclaring into a **fresh** place (the old place's teardown still runs at scope exit and finds the null it expects). That is the whole relaxation: reuse after an explicit end. Redeclaring while live stays an error. Legal: `y := own x` then `x := …`; the REPL's `drop x` then `x := …` (session body ordered like any other).
- **Rejected, to stay declined:** one-line `x := f(own x)`. **Why:** `:=` declares its name before parsing its value (self-reference needs it), so the right `x` is the new placeholder and the check already fired.
- **Rejected, to stay declined:** dead after last use. **Why:** needs the rest of the scope in view, which the eager one-pass parse forbids.
- **Ruled:** 3 September 2026.
- **Source:** DESIGN.md l.265

### Metadata has three homes, by frequency and origin
No fact wraps a node. **Why:** a wrapper would make every type read yield the wrapper, and one in the value chain breaks the prime invariant that the type defines how the value is read.
1. **Universal and tiny:** empty; the common case stores nothing.
2. **Deviations the user writes** (`pub`, per-field `mut`, capability and backend modifiers): read off the declaration's structure at resolution, the lookup `.` already does. Gates among them are cached on the binding beside the range; the gate node on the declaration is the truth, the binding the derived fast path (as the range is to the killing node); one lookup reads both.
3. **Deviations analysis infers** (a lifetime extended by escape, a borrow state): reflectable nodes. An inferred fact about a named identity (non-default lifetime) lives on its binding; one about a constructed node lives in the fact node pointing at it. Relational and event facts (a borrow references its target; a move relocates a scope-level drop node) are reachable nodes anyway, nothing on the target.
Default: the scope tree gives lifetime (enclosing scope) and visibility (own scope and descendants), nothing stored, no gate word, node stays 16 bytes.
- **The rule:** a deviation the user writes lives in the declaration; one analysis infers lives in a fact node. A fact that can be either (annotated vs inferred lifetime) enters by its own door.
- Source positions are not a facet: they are the span a node was parsed from, in a derived source map (one-to-one with source, rebuilt, absent for runtime-made nodes). Fine-grained permissions ride the reference (*Reference-granular permissions*).
- **Ruled:** revised June 2026, replacing the wrapping design.
- **History:** home (1) was four construction-phase tag bits per node, superseded 7 September 2026 (`undefined` is a null slot, writability a gate). A derived id → metadata index superseded 8 September 2026: no address-keyed index; facts come from name or path. Gates: separate identity binding 5 September, one binding 7 September.
- **Source:** DESIGN.md l.267

### Lookup rides the scoped walk; the index only speeds things up
Lexical scoping makes a declaration an *ancestor* of every use, so a top-down walk (push on scope entry, pop on exit) has the metadata in hand; the batch pass needs no index. References skipping ancestors (types, functions, definitions elsewhere) are the `'static`/default cases. Out-of-walk access (re-check after an edit, a hover) re-resolves the spelling at that position via source map and trie, or walks the written path.
- **History:** derived id → metadata index for that case, superseded 8 September 2026.
- **Source:** DESIGN.md l.269

### A uniform model is not uniform storage
Every identity *has* a permission and lifetime; almost none *store* one. The node is type + value (16 bytes; handle = address). Metadata only on deviation; defaults from the scope tree (permissions on scopes, lifetime from the enclosing scope, dropped at its end). The common case (immutable `'static` definitions: operators, types, stdlib) shares one default, readable by all, no lifetime tracking, no drop node, no borrow state.
- **Why:** storage scales with *deviations*, not identity count; that makes a uniform model affordable.
- **Source:** DESIGN.md l.271

### A declaration has a stable identity across edits, separate from its content hash
The address is per-run and the content hash changes every edit, so neither follows *the same declaration*. The stable handle is nominal: the spelling it was declared under (optionally a regex, for pattern-recognized identities like numeric literals) plus the declaration context separating same-spelled identities. Stable under value edits, changes only on rename: the complement of the content hash.
- A cache keys by content (should miss on change). A proof in progress, a derived-fact cache, a collaborator's cursor follow the declaration, so key by nominal identity.
- Device-agnostic: systems sharing a root context resolve it alike, so cross-device references and re-parses re-attach metadata to the right node.
- A derived accelerator, not a node field: interpretation uses addresses; the nominal identity serves serialization, cross-device exchange, re-parse correspondence.
- Named declarations only; anonymous subexpressions re-associate by structure or content.
- For a CRDT over the graph, it is the human-stable half of a node id; a globally unique origin-attributed part (replica id + logical counter) is added, to separate independently created same-spelled declarations and give anonymous nodes an id.
- **Ruled:** direction reached in discussion, July 2026.
- **Source:** DESIGN.md l.273

## Standard library and ecosystem strategy

### The standard library is built before release
To avoid the fragmentation of thin-stdlib languages: by release, the standard ways to do common things are decided, documented, idiomatic and tested by real programs. *Release* means v1.0.0, the stability promise. Previews (v0.x, v0.1.0 first) ship almost no stdlib, labeled not-to-build-on; completeness gates v1.0.0, where fragmentation risk begins.
- **Ruled:** in discussion, 31 August 2026.
- **Source:** DESIGN.md l.277, l.279

### What it contains
Collections and the iterator protocol `for` desugars onto; Unicode-aware strings and parsing; numerics incl. arbitrary precision and cost functions for stability-aware rewriting; symbolic maths (expressions, algebraic rule sets, differentiation, integration, polynomial and linear algebra, domain support, verified rules where possible); allocators and smart pointers; concurrency and async primitives; I/O, networking, filesystem (files and folders as Logic Graph values); verification utilities (SMT, refinement predicates, inductive types, common tactics); the compiler-extension framework; the documentation generator.
- **Source:** DESIGN.md l.281

### One canonical solution, batteries included
**Single canonical solutions, opinions formed, batteries included, idioms documented.** Competing libraries may exist; the stdlib defines idiomatic Logos, and the language is opinionated enough to back it.
- **Source:** DESIGN.md l.283

### Completeness, not a clean minimal core, stops fragmentation
A minimal core causes dialect sprawl: each gap is filled downstream, differently. Scheme (cleanest core) fragmented worst; Common Lisp and Python (complete, canonical) held together. Cleanliness buys something narrower and real (no wart-patching extensions), but the load-bearing protection is a base so complete nothing is missing. The trap: a beautiful minimal core whose gaps invite a thousand well-meant fillings.
- **Ruled:** recorded in discussion, 31 August 2026.
- **Source:** DESIGN.md l.285

## Tooling philosophy

- **LSP first.** A Logos-written LSP server (highlighting, errors, autocomplete, go-to-definition, hover, refactoring) in any LSP editor; users keep their editors. Richer than elsewhere because the graph holds so much. Comes after v0.1.0 (see the preview milestone).
- **Standard-protocol debugging and profiling:** DAP, standard samplers, flamegraphs.
- **Logos-native structural editor and IDE, long-term.** Works on Logic Graphs: semantic editing, principled refactoring, customizable in Logos, can ask "update all related occurrences?" with confidence. Far smaller than equivalent IDEs because meta-reflection replaces separate AST, analyzer, refactoring and language-server stacks. Not on the critical path; LSP covers the near need.
- **Documentation generation is a flagship.** Types, signatures, doc comments, examples, capability declarations and proofs are in the graph, so docs show verified examples, proof obligations, capability summaries; the generator is customizable Logos.
- **Source:** DESIGN.md l.289-292

## The IDE as platform, and the reflective web

Direction, July 2026. Far-horizon: needs the base language, compiler and structural editor first; not on the critical path. The IDE is Logos programs over the graph; with I/O and portable backends, the same reflective structure can grow, without a rewrite, into a shareable, networked, rendering platform: a browser reached from the tooling end. (Source: l.296, l.298)

**Text or graph, because the graph is primary.** A use is a reference (its binding, since 8 September 2026), an edge, not a spelling re-resolved on the fly. Text and graph are two renderings of one structure, not two artifacts kept in sync; the editor shows any region as glyphs or as nodes and edges, per region. Per region stays readable (a whole-program view is a hairball), and arrows are references already in the graph. In a text language edges must first be recovered by parsing, so this would be a sync problem there. (Source: l.300)

**Received code is checked before it runs.** A shared unit (code, data, interface) can carry checkable proofs of its behavior, proof-carrying-code style (no network, reads only what it is handed, terminates). The receiver's kernel checks cheaply before running, and the unit runs under the capability model with no ambient authority. This answers the web's gap: received code trusted by default, never inspectable. Reach: verifiable *behavior* and *provenance* (which axioms or sources a result rests on, the proof's branch), not the truth of arbitrary claims (a proof shows only what is specifiable), and only for content built in the system (legacy content has no proofs). (Source: l.302)

**The web is reached by compiling to WASM, not interpreting inside it.** WASM is already a listed backend. The platform uses the language-neutral primitives: component model and WASI (module interop), WebGPU (compute, and drawing the interface directly), QUIC with HTTP/3 and WebTransport (transport), OPFS (local storage), drawing its own interface instead of reimplementing the DOM and web APIs. **Why not ship the interpreter:** it stacks the graph-walker's cost on WASM's, and pointer-chasing is what WASM does worst. Interpreter speed is off this path anyway: it runs cold mutable code; hot regions compile. (Source: l.304)

**Real-time collaboration, because source is truth and identities are stable.** Each device rebuilds the graph with its own addresses, so systems exchange *source* and resolve it locally. Collaboration is a CRDT document, stored in OPFS, synced over WebTransport, at two levels: near-term, a sequence CRDT over source text with each device re-parsing (no graph-level identity, existing libraries; nominal identity and incremental re-identification keep per-node metadata across re-parses); later and harder, a CRDT over graph structure for collaborative structural self-modification (needs the origin-attributed id on every node; complicated by cycles). Promotion (unparse a graph edit to a source diff, re-check, commit under gate) keeps graph edits anchored to source. (Source: l.306)

**The Smalltalk image's failure mode is designed out.** Smalltalk was productive but lost on diffable, mergeable, collaborative, deployable, interoperable, because of the image: a live mutable binary object graph as source of truth, with no stable, comparable, portable form. Logos: source text is canonical (diff, merge, collaboration ride on text and CRDTs keyed to stable identity, not heap comparison); sealing collapses a region to a bare native artifact (the small deployable Smalltalk lacked); foreign code as a declaration and standard backends prevent an island. Remaining risk is cultural: everything-in-the-graph must not become everything-must-enter-Logos, how the ancestor became a walled garden. (Source: l.308)

## Englogos: a Logos-native human language

Direction, July 2026. Far-horizon: needs the proof layer and ecumenical system; not on the critical path. A first surface sketch exists (August 2026, language_sketch.logos). Learnability, speakability and the wider lexicon are undesigned. (Source: l.312)

**The inverse of hosting.** Hosting takes natural language as it is (irregular, ambiguous) and represents what cannot be resolved. Englogos is a constructed human surface whose grammar is Logic Graph constructors from the start: regular by construction, its whole rule set queryable any time. No new substrate: the parsing doctrine ("surface forms as irregular as natural language are in scope") applied to a surface designed to be regular. (Source: l.314)

**Target: Humboldt's property, engineered: finite means, no exceptions, dense.** Human languages use finite means infinitely, but their rules are implicit, unlistable, full of exceptions kept only socially. The substrate's refrain is one rule, no exceptions (one evaluation rule, one reader–writer rule, one resolution rule, one dyad). Density comes from each rule's generality, never from irregularity. (Source: l.316)

**Exceptions dissolve: grammar is data, irregularity pays admission.** Every token, construction and `parse_rank` is a queryable node, so no implicit rule hides. An irregular form is allowed only as an explicit constructor, admitted under the recognizer's proof obligations (terminates, deterministic, no effects) and disambiguated by a fixed order, never a statistic. An exception is just another first-class rule, visible to all; the category does not exist. (Source: l.318)

**Generativity is two-level.** The finite kernel generates sentences and also grammars. Constructors are Turing-complete, so constructions and vocabulary (new identities) grow while every extension is expressed in and checked by the fixed core. Lexicon and grammar are open; the kernel never moves. (Source: l.320)

**Meaning belongs to the proof layer.** A declarative sentence denotes a proposition; "executing" it is checking it. Empirical claims enter as asserted axioms with provenance, committing to ecumenical branches. Epistemic status (proven, refuted, plausible to a degree, unknown) is assigned lazily and revisably by the coherentist layer, never the parser. Englogos only adds an authored, regular sentence-to-proposition map, so candidate sets are small or single by construction. (Source: l.322)

**Density through underspecification stays legitimate.** An underspecified form parses to the candidate structure it denotes (alternatives as plain value structure, per the hosting ruling); resolution is lazy, at a use site, surfacing `?` where demand outruns knowledge. **Rejected, standing:** driver-level backtracking; searching underspecification for a winner the source does not determine. (Source: l.324)

### First surface sketch: a split copula, and tense as a stack of reference points
A sketch, not stable (August 2026; examples in language_sketch.logos). Recorded because "no vocabulary exists yet" no longer held. Two decisions are fixed: the copula split and the tense stack.
- **Copula:** `be` assigns or creates (directional: stipulation or coming-into-being); `is` affirms or checks. Identity with `is` is symmetric, `be` never; so the checker tells an axiom being installed from a proposition to prove.
- **Tense stack,** root = speech time: `la`/`il` push a point past-of/future-of the current one; `o` resets to root permanently, `so` for one word; `sa` pops one point for the rest of the sentence, `se` for one word.
- **`o` is a keyframe:** a reader joining mid-stream resyncs at the next `o`; a corrupted marker damages at most the span up to it. «he olaeat and she olasang» leaves two pasts deliberately unordered, each `o` re-anchoring before its `la`: the differential encoding's fragility bought back for one letter, at the writer's choice.
- **Markers compound left to right,** an ordered chain, never a set (`illa` = a past of a future). **Differential:** each verb marks only its offset from the previous verb's point: "he said she had told him they would finish" = `lasay latold ilfinish`, where English re-derives each clause from speech time and piles up auxiliaries.
- A bare verb sits at the stack top; a quantifier over times still binds it (bare `is` under «in all time» is bound, not deictic).
- **`?`, spoken «que», asks whatever slot it fills:** a thing, person, time («when que», «the time lailis que» = a future-of-a-past), or as `is`'s argument a clause's truth («she laleave ois que» = yes/no question). It is *Density through underspecification*'s `?` in the speaker's mouth.
- `often` is a frequency quantifier centered on the current point: the habitual needs no aspect axis.
- `when P` commits P will hold at an unknown time; `if P` accepts P may never hold; neither moves the stack.
- **Negation** scopes over what follows and focuses the next word. With individual arguments, position changes only the evoked alternative, never the truth; with quantifier words, position is scope: «not often eat» = rarely, «often not eat» = frequent skipping.
- **«.» resets nothing:** the stack persists across sentences, the keyframe is the only resync; a document is a delta stream with keyframes. So `o` is a one-way door, written once the points below are finished; mid-sentence `so` and compounded `se` reach now or an outer point without burning the chain.
- **Roots never inflect:** `laleave`, never `laleft`; a host-language irregular form is an error.
- **Modality:** `must` = necessity, `should` = expectation, differing in strength as «always» to «often» (obligation always met vs often unmet). A modal has its own time: «he must lais lying» = necessity now about lying then.
- **Progressive:** `is` over the `-ing` state: «lais crossing» affirms an in-progress state at a past point; states never imply completion, so «was crossing, never crossed» is consistent (the imperfective paradox dissolves lexically, the `die`/`dead` split made productive).
- **Possession is location:** «ears at him»; `at`'s one contextual-restriction reading does the deleted `hav`'s work.
- `almost` is the proximative: underway or imminent, and did not happen.
- **The stack is the index half of names vs indices:** named anchors over `before`/`after` stay the general mechanism for deep or long-range reference; short forms are sugar. The stack is a pure function of the words to the left, so parsing stays the one tape pass with no new parser state (a constructor reads back on the tape).
- **Open:** `-ing` is the first suffix and the participle's composition is undesigned; generic `when` («ears at him turn red when he lie») vs its episodic ruling; no words for quantifiers over times («never», «ever»); trailing `not` in live examples unreconciled with rightward scope.
- **Rejected, to stay rejected:** tense morphemes carrying counterfactuality (English's fake past). **Why:** a counterfactual marks an ecumenical branch, and an antecedent's falsity is cancellable implicature, status assigned by the proof layer, never the parser.
- **History:** a push-simultaneous `o` deleted as redundant (moved depth, never time). Interim `paoil` as "eternal" superseded: its set reading cannot survive sequence semantics; eternity is quantification over times, not a marker.
- **Source:** DESIGN.md l.326

### The surface converges on English, and speakability gates admission
As close to English as regularity allows, because adoption needs familiarity. New machinery only where English has no exception-free device (tense stack, copula split, `que`).
- **Rejected:** deliberate foreignness as signage (alien surface wherever English is irregular, so readers distrust their intuitions). Uninflected roots stay as the regular device, not a warning label.
- **Accepted risk:** false friends (parses, but not to the reading the writer's English meant). **Why:** the surface is machine-checked; the reading echoes back at write time, so a wrong guess shows at once.
- **Speakability gates every identity at admission:** a word is said aloud before it is ruled; meanings that must not be confused get phonetically distant forms; a marker set is judged as a sound system.
- **Open:** that ground covers tool-mediated writing; spoken use has no checker and its discipline is undesigned. First standing case: `so`/`sa`/`se` are one vowel apart where a misheard marker corrupts to the next keyframe; respelling open.
- **Ruled:** in discussion, August 2026.
- **Source:** DESIGN.md l.328

**A surface option, not an ethos.** Englogos is one surface among several (hosted natural languages, ordinary Logos), never required for entry (the walled-garden warning). The substrate's guarantees end where human-language design begins: learnability, speakability, morphology, vocabulary are their own disciplines, which is much of why this is a direction, not a design. (Source: l.330)

## Chain programmability: constrained targets as backends

Direction, July 2026. Far-horizon: needs backends beyond Cranelift, the rewriting engine, the verification strata; not on the critical path. Prompted by covenant programmability on UTXO chains (Kaspa's Toccata upgrade, SilverScript), recorded chain-neutrally. (Source: l.334)

**A chain VM is a gate-constrained backend, not a new subsystem.** A backend carries its lowering rules and a fail-closed gate predicate over the scopes it accepts (*Backends are identities*). A script or covenant VM is such a target: tiny, deterministic, fee-priced, rejecting most of the language. The gate's demands (determinism, termination, no I/O) are native vocabulary: capability tracking and totality obligations, stated and checked. Hosting a chain language's full semantics as a graph dialect stays the separate, heavier option; the in-grain path is a Logos surface lowered through a chain backend. (Source: l.336)

**Fee schedules are cost functions.** The rewriting engine extracts the cheapest proven-equivalent form under a user cost function, so a chain backend ships its fee schedule (gas, script pricing) as one and gets fee minimization as ordinary extraction: safely aggressive, each rewrite carrying its equivalence proof, where a miscompile loses funds. (Source: l.338)

**The domain rewards the verification strata.** Contracts are immutable, adversarially attacked, and failure is unbounded loss: the reverse of the trade-off that makes proofs a luxury. Exploit classes (reentrancy, overflow, access control, accounting errors) are mostly statable invariants. Unlike bolted-on specialist verifiers: same language, same graph, from refinement to dependent proof. Comptime supplies flexibility the target lacks: full Logos runs at build time to generate the small artifact the gate admits. (Source: l.340)

**The reach is exact.** The VM is a hard ceiling; the gain is authorship (correctness, cost), never on-chain expressiveness. Proofs show only what is specified: wrong specs, oracle and economic attacks, key theft untouched. A ZK execution proof differs from a compile-time proof about the program, though a zkVM fits the backend interface. (Source: l.342)

## Durable state: checkpoints, caches, and the store

Direction, August 2026. Nearer-term than the other direction sections, not on the v1.0.0 critical path. Store file format and resource re-acquisition protocol undesigned. (Source: l.346)

### A durable store, in files, managed by Logos code alone
Things that outlive a process live in a file-backed store keyed by content, structure or nominal identity, never per-run address ("anything that must survive a session (caches, compiled artifacts, certificates) keys by content or structure, never by id"). Cross-process identity = the graph-CRDT one: nominal pair plus origin-attributed part, covering anonymous nodes.
- **Durable references (direction):** the type defines how a value is read, so a durable pointer type resolves a stable id where `@T` dereferences a per-run address; the type, not tag bits, says which store. Format undesigned.
- **Seed:** owes only file syscalls, already in the `native` floor. Formats, manifests, compaction, policy are stdlib or third-party Logos; the manifest is graph values.
- **Two tenant classes:** **re-derivable** (cached compilations, proof certificates, derived facts): deletion loses only time. **Authoritative** (checkpoints, later database data): the only record of what the outside world told the program.
- Databases (hosted query surfaces as constructors, storage engines behind one interface like allocators, planners as the rewriting engine under I/O cost functions) are a later tenant, not a subsystem.
- **Source:** DESIGN.md l.348

### A checkpoint is saved task state
A paused task *is* its graph state at burst boundaries; a checkpoint serializes that, every `bcode` nulled. Resume builds a fresh graph (addresses → file offsets out, rebuilt in), starts interpreted; recompiling is a cache lookup.
- Saves only where code says. A library may wrap a timer around the save; the runtime never checkpoints behind the program's back (same law as constructor-inserted teardown and boundary placement).
- Plain memory serializes transitively. An outside-world value (open file, connection) refuses: a checked error at the save site, findable because its constructor declares what it is.
- A task waiting on the outside is saved *as waiting*; on resume it re-asks (re-request, re-arm, re-register with its reactor, the user-definable layer). Tolerating re-asks is the program's job, as with cancellation.
- **Source:** DESIGN.md l.350

### Cached machine code comes back as foreign code
`@exec` has exactly two licensed mints; the cache adds none. A cached compilation is written as an ordinary dynamic library and loaded through the foreign-declaration door, so the platform loader (and OS code signing) does the trust work.
- Content keys and fingerprints guard against corruption and staleness, not attackers: whoever writes the cache can write the source, so file permissions secure both.
- Keys come from the per-subtree use-sets (content plus what it resolves to); an edit changing no dependency (a comment) invalidates nothing.
- The cache persists what *Versioned scopes* models (a scope's proven-equivalent compiled alternatives, now discarded with the scope): a durable home, not a new kind of thing.
- **Source:** DESIGN.md l.352

### Rejected, to stay rejected (durable state)
A third `@exec` mint for cache loads; runtime-automatic checkpointing in any form (timers are libraries over the explicit save); implicit persistence of I/O-backed values; a separate argument channel or mini-language (the command line is Logos source).
- **Source:** DESIGN.md l.354

## Feasibility and effort

| Component | Effort / risk |
|---|---|
| Bootstrap seed (Rust) | Bounded; the hard part is restraint, small enough to audit. |
| Base language (self-hosted) | The bulk, but well-understood (borrow checking, inference, data structures); strongly sped up by LLM-assisted implementation. |
| Rewriting engine | Bounded; egg-style equality saturation adapts to the graph. Cost functions and rule authoring take longer. |
| Symbolic maths stdlib | Most uncertain. Differentiation, basic integration, polynomials, simplification early; Mathematica-level is a long effort, not needed: an 80%-case library with compile-time symbolic evaluation beats another runtime CAS clone. |
| Refinement types + SMT | Established (Liquid Haskell, F\*, Dafny); integrating SMT discharge with the base type stratum plus tooling: real but bounded. |
| Full dependent types + proof terms | Deepest, longest; Lean's foundations took years. **In v1.0.0** (5 September 2026: the release is the completeness promise; verification left out is a gap downstream fills). Foundation present from the start; implementation grows toward it. |
| Backends | Cranelift, LLVM well documented; custom backends (embedded, WASM, GPU) smaller given a good interface. |
| Standard library | Lifelong; v1.0.0 subset: collections, numerics, I/O, concurrency, basic symbolic maths, verification utilities. |
| LSP server | Less than elsewhere; information already in the graph. |
| Documentation generator | Modest; rendering and cross-references. |
| Structural editor / IDE | Large, far smaller than elsewhere; off the critical path. |
| Debugger/profiler | Standard protocols first, Logos-native later. |
| Compiler-extension framework | Partly free from meta-reflection; conventions mature with the stdlib, which shows the patterns. |
| Verified demonstration projects | Verified kernel module, crypto library, or compiler pass; research-grade; earns credibility in the verification niche. |

- **History:** dependent types were "not needed for an initial release" until 5 September 2026.
- **Source:** DESIGN.md l.358-373

### The v0.1.0 preview leads with "the language defines itself"
A new operator or keyword defined in ordinary Logos, used in the same file, compiled with `f.compile()`. Critical path in order: (1) eager-segment driver convergence; (2) the tape's four affordances as Logos-reachable identities; (3) Logos-written constructors run during the parse; (4) `lex «…»`; (5) the demo file. Not v0.1.0 work: verification (proofs, refinements), the borrow checker, the rewriting engine.
- **Ruled:** in discussion, 31 August 2026.
- **Source:** DESIGN.md l.375

### `lex` stays on the v0.1.0 path
Although the reshaped demo never quotes text.
- **Why:** splicing text-built fragments is the macro layer's only tool; without it v0.1.0 would show constructors that only shuffle cells already on the tape.
- **Ruled:** re-affirmed 5 September 2026.
- **Source:** DESIGN.md l.375

### How a Logos constructor builds its node (step 3's first question)
A `run`-carrying node holds its operands as named fields the constructor writes. The write never reaches the identity the cell pointed at: a fresh node of the assigned type replaces the cell (*The scope's constructor is the driver*). Current spelling (l.207): stamp `tape[0]:type = ^`, then `tape[0].lhs = tape[-1]`.
- **Why the type write initializes:** Thobias, 9 September 2026: "when assigning the type of a dyad the value should be automatically initialized so that .operands is valid and available".
- **Ruled:** 9 September 2026; named fields 16 September 2026.
- **History:** 9 September: `tape[0]:dyad.type = ^` then `tape[0]:dyad.value.operands.append(tape[-1] and tape[1])` (the and-group distributing so both append, *The proof layer* 7 September; that and-group ruling stands on its own); the operand record was the null-terminated run call arguments travel in. 16 September: named fields. 17/18 September: `this.lhs = tape[-1]`, `this` placed by `tape[0] = this`. `this` gone 26 September (l.207); `:dyad.type` → `:type` 23 September.
- **Source:** DESIGN.md l.375 (current spelling from l.207)

### The LSP server comes after v0.1.0
- **Why:** the preview promises the demo and the language defining itself; a server is tooling the graph makes cheap later; gating on it delays the invitation for no language evidence.
- **Ruled:** 9 September 2026.
- **History:** before: "A thin LSP server gates v0.1.0's public release, not the demo."
- **Source:** DESIGN.md l.375

### Staged out of v0.1.0: error values, and writing a name's dyad view
v0.1.0 has faults only and `-> void` constructors (*Error handling*). The dyad view is fully readable, writable only where the demo needs it: a constructor writing a tape cell nothing has read yet, `tape[k]:type = T`. Writing a declared name's view, `x:type = f64`, is the checked error; gate rulings decide after v0.1.0 who may write where.
- **Why:** it would rebind the name's type or storage under every reader and static range: bug-prone, no preview gain.
- **Ruled:** 2 September 2026; name-write refusal 9 September 2026.
- **History:** 2 September: view "fully readable and writable". Spelled `:dyad` (`tape[k]:dyad.type = T`, `x:dyad.type = f64`) until 23 September 2026.
- **Source:** DESIGN.md l.375

### Gates before v0.1.0: `pub` stays and `mut` lands
- **Why:** Thobias, 20 September 2026: gate machinery is best tested early, and `pub` already carries imports. #64 closed without the change.
- **Ruled:** 20 September 2026, Thobias.
- **History:** 4 September 2026: v0.1.0 has no gates (no `pub`, no `mut`; all readable and writable; `=` writes any declared name; an importer sees every top-level name), because a gateless seed is smaller; `pub`-only exposure, `mut` as type modifier and the fail-closed default were to land together after v0.1.0. The seed's `pub` gate slot and pub-only import tests were a divergence pending removal until 20 September.
- **Source:** DESIGN.md l.375

### The v0.1.0 demo: a user-defined power operator `^`
`^` is defined with `type`: right-associative; `parse_rank` relative, `*.parse_rank + 1`; a Logos-written constructor builds one node typed `^` whose fields `lhs`, `rhs` come from `tape[-1]`, `tape[1]` (written by name), removes the two cells, and defines the node's `output_type`; `run` is a bare body over `lhs` and `rhs` computing the power (*Execution is function application*). Then `f := fn (x := i32 ?) -> i32 ( x ^ 3 + 1 )`, `f.compile()`, `f(2)`. File: identities/power.logos.
- **Pinning constraint:** v0.1.0 constructors build **existing node kinds**, since a new kind needs a lowering rule per backend (post-v0.1.0). A node whose type carries a `run` is the existing call kind: `f.compile()` lowers `x ^ 3` as an ordinary call to `^`'s code.
- **Step 3 must expose from the seed:** `tape[k]` read and write, `remove`, `dyad (type, value)` construction from Logos, `tape` as a word inside `parse` (slot body with no parameter list), `.parse_rank` as a comptime field read, a type's `run` slot used by the evaluation rule and `compile`.
- **Rejected:** a separate Logos-written `pow` called through `^` (2 September form), deleted as old thinking. **Why:** the operator is the operation; `fn` would only be shorthand filling the parse slots `^` fills by hand.
- **Ruled:** pinned 2 September 2026; reshaped 4 September; respelled 16 September (fields, `output_type`, bare `run`).
- **History:** computation first in a `code` slot as an ordinary function (now `run`). Parameter first `x : i32` (superseded colon form), respelled the same day; the `:`-as-sugar door of *Declarations are immutable by default* stays open but unused. "`fn`-typed parameters carrying a tape" superseded 17 September 2026 by `tape` as a word in `parse`.
- **Source:** DESIGN.md l.375

### `^` takes floats and any real exponent
A float on either side, or a literal fraction exponent, makes `output_type` that float (`f64` over `f32`); else `lhs`'s type. A whole exponent multiplies; a negative whole one divides (1 / lhs^n), refused as the checked error for a whole-number result (2 ^ -1 is no integer). Other exponents go through `exp` and `ln`, ordinary Logos functions in identities/power.logos; a negative base there is the checked error.
- **Why:** the demo shows a whole operator defined inside the language, so its maths is Logos too; nothing native added.
- **Ruled:** 26 September 2026, Thobias: "make definition of ^ more complete so that it supports floats as well? and if lhs or rhs is float then output_type has to be float as well"; then "Any real exponent", negative whole exponents as 1 / lhs^n, exp and ln "written in Logos".
- **Source:** DESIGN.md l.375

### Accelerants and decelerants
**Accelerants** earlier projects lacked: LLM-assisted implementation; mature infrastructure (Cranelift, LLVM, egg); well-understood foundations (dependent type theory, SMT, borrow checking); a decade of PL research. None makes it trivial; each cuts effort versus inventing the piece.
**Decelerants:** unification is genuinely new among systems languages, so some problems appear only in implementation; the everything-in-the-graph commitment is tested by every feature and needs sustained discipline; verification is where surprises are likeliest.
- **Source:** DESIGN.md l.377, l.379

## Closing

The vision is large, and honest about it. Each piece has working precedents (Lean 4's self-hosting, MLIR's layered IR, egg's equality saturation, Rust's borrow checking, Smalltalk's malleable system); the contribution is unifying them, not inventing from scratch. The risks are real, but the architecture is sound and coherent.
- **Source:** DESIGN.md l.383
