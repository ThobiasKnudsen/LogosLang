# Logos v1 Seed — Implementation Plan

*This file is the **build plan** (how and in what order). The **why** lives in `DESIGN.md`; the **target graph spec** lives in `language_sketch.logos`. Keep design rationale out of here — link to DESIGN.md instead.*

## What v1 is

v1 is **done** when a small Logos program runs **both interpreted and Cranelift-JIT-compiled**, with the interpreter result used as the correctness oracle for the compiled result. The canonical smoke test is `a = a + 1` (sketch line ~222), then a program with a `struct`, a recursive `fn`, and an `if`/`while`.

**In scope (v1):**
- The node model (`dyad` = `{type, value}`, 16 bytes, tag bits) and a node store (any allocator handing out stable addresses; address = id — see DESIGN ›The store is keyed by address‹).
- The hand-built **core graph**: `type`, `fn`, `struct`, `array`, `mut`, `scope`, primitives (`i32`…`f64`, `rational_number`, `bool`, `string`, `void@`, `exec@`), `Type:Type` self-loop.
- **`hashtable`**: backed by Rust's `HashMap` for speed — its operations are `native` (opaque to Logos), but its stored entries are reflectable LG data. (struct `names` uses it.)
- A **regex-trie lexer** (port of [`regex_trie.zig`](https://github.com/ThobiasKnudsen/Logos/blob/zig_pivot/src/lang/regex_trie.zig), using the Rust `regex` crate) — the single string/regex→identity index, extended on every declaration.
- A **one-pass, self-directing parser/elaborator**: a deferred-reduction operator-precedence driver over an explicit **parsing tape** (not Pratt, whose hidden recursion stack cannot express token-rewriting operators), running native `constructor`s, on-demand lex/parse interleaving, scopes + name resolution.
- **Access via `gate`s** — the single permission primitive `pub` and `mut` are sugar over. A `gate` is a **fail-closed predicate** returning `true | false | ?`: access is permitted only on `true`; `false` and `?` both deny, with `?` retained as a standing obligation. Checks are **lexical/static** — decided from the viewer's scope chain at elaboration time and erased at runtime, so there is **no ambient authority** (an indirect call can't launder access; a callee gets access only via an explicitly handed reference). A gate's requirement propagates *backward* through the call graph as a static obligation discharged at the granting scope, and stays analyzable from any code section. `scope` is the grant-bearing core identity. v1 builds the chokepoint + the lexical tier + the typed `?` seam and routes name resolution/visibility through it — **no prover** (the borrow checker and proof layer bolt onto `?` later; see Out of scope and DESIGN).
- **`rational_number` literals**: numeric literals lex as `rational_number` (an uncommitted comptime-number that molds to any int/uint/float); the `+`/operator constructors mold a literal to the *other* operand's type (i32 for the smoke test). v1 builds only the literal-carrier + coerce-to-concrete path (see DESIGN). *Why:* one literal form serves every numeric type with no suffixes.
- The **`fn` execution model**: `compile` (Cranelift → `bcode`) and `run` (jump to `bcode` if present, else walk `body`); interpretation is `run`'s null-`bcode` path, not a separate function; stack calling convention; ~30–40 native endpoint operations.
- **comptime**: `compile()`+`run()` and `run { … }` executing inline during the one pass.
- **Structural mutability of user code** (Option B; the seed itself stays frozen): a user `mut (…)` region is structurally rewritable — by a `constructor`/macro at parse time and by `graph_mut` at run time. Mutation is **unsafe** in v1 (no borrow checker). The seed maintains the name index automatically on every edit and is fail-closed on null `bcode`; `bcode` freshness of edited compiled code is otherwise the programmer's job. Details + the one open decision in §Mutability & deopt.

**Out of scope (later, not v1):**
borrow checker (the stronger access tier — bolts onto the gate `?` seam), refinement/SMT types, dependent types & proof layer, the rewriting/equality-saturation engine, adaptive hot/cold promotion, standard library, LSP/IDE, incremental re-lexing, per-subtree use-sets / proactive dangling-reference detection (a reference into a deleted scope surfaces lazily, as a resolution error at the next interpretation), automatic deoptimization (`bcode` freshness is programmer-managed in v1 — see §Mutability & deopt), multi-backend (LLVM/WASM/GPU), arbitrary-precision `rational_number` arithmetic (the both-operands-uncommitted, number-as-string path). v1 must not *block* these (DESIGN already records the hooks), but builds none of them.

## Honest effort read

Bounded. Order of **5–10k LOC Rust**, weeks-not-years solo, most of it LLM-friendly grind. Spend personal attention on the three meta-circular spots (flagged ⚠ below); let LLMs handle the rest.

- **LLM-grind (high volume, low novelty):** the trie port, the ~30–40 endpoint natives, the per-type constructors, the per-endpoint Cranelift lowering rules, error/diagnostic plumbing.
- ⚠ **Needs your care (novel, meta-circular, weak LLM priors):** the cyclic core-graph bootstrap; Cranelift control-flow/SSA lowering; the self-extending-trie × one-pass interleaving.
- **Real risk is correctness of the meta-circular core, not line count.** Mitigation: smoke test end-to-end first, interpreter-as-oracle for the JIT, grow outward.

## Crate layout

```
seed/                           # single binary+lib crate
  Cargo.toml                    # deps: regex, cranelift-{jit,module,frontend,codegen}
  src/
    dyad.rs                     # dyad cell, tag bits, void@/exec@/dyad@ handles
    store.rs                    # node storage; address = id (any stable-address allocator)
    core.rs            ⚠        # hand-built cyclic core graph (Type:Type, primitives, fn/struct/array/mut)
    lex/
      trie.rs          ⚠        # regex-trie: 256-array literal path + regex-crate leaves; leaf = list of id_context; incremental insert
      token.rs                  # token = {text/denoted}; tape-resident, not a persistent record (source spans live in a derived source-map)
    parse.rs           ⚠        # the parser: parsing tape + scope stack + name resolution (id_context filtered by O(1) open-scope membership, no shadowing) + the lexical `gate` access check (fail-closed, erases); deferred-reduction operator-precedence driver; on-demand lex; runs native constructors
    interp.rs                   # run's null-bcode body-walk (tree-walk) + stack frames
    endpoints.rs                # ~30–40 native run() ops
    compile/
      types.rs                  # LG type -> Cranelift type / layout
      cfg.rs           ⚠        # structured control flow -> blocks/brif/jump, SSA via Variables
      lower.rs         ⚠        # per-endpoint lowering; whole-fn body -> one IR graph -> bcode
    error.rs                    # void! / (T|Error), span-aware diagnostics
    main.rs / lib.rs            # driver: parse -> (interpret | compile+run)
```

## Build order (each phase ends at a runnable, testable checkpoint)

**Phase 0 — Node + store.** `dyad`, tag bits, node store (address = id). *Test:* construct/read nodes, follow `type` to the self-loop. *Small, easy.*

**Phase 1 ⚠ — Core graph bootstrap.** Hand-build the mutually-recursive identities as one cyclic structure (`type`, `fn`, `struct`, `array`, `mut`, primitives). Mirror `language_sketch.logos`; the sketch is the spec, not parsed yet. **Precondition — close and validate the sketch first:** every identity the smoke tests and constructors reference must be *declared* in the sketch (Logos-source declarations, not graph expansions — the seed builds the graph in Rust), and the sketch must be internally consistent (no identity defined two incompatible ways; operator usage matches DESIGN's `:`/`:=`/`=` semantics). A self-inconsistent sketch passes its own assertion and surfaces downstream as something baffling, so it is not trustworthy as the oracle until closed. *Test:* assert the graph shape against the sketch (the planned seed-vs-spec check, DESIGN 17). *Fiddly — cycles. Your attention.*

**Phase 2 ⚠ — Lexer.** Port `regex_trie.zig` to `trie.rs` using the `regex` crate for leaves; literal declarations are pure-literal inserts, regex tokens recompile one node. Each leaf stores a **list of `id_context`** (`{identity, scope}`), not one value; a use resolves against that list by open-scope membership. Tokens are tape-resident (`{text/denoted}`), not a persistent record; source spans live in a derived source-map. *Test:* lex `a = a + 1`, lex `«…»` strings and `#` comments, insert a new token then lex a use of it. *Mostly grind; the incremental-insert correctness is the careful bit.*

**Phase 3 ⚠ — One-pass parser/elaborator.** Deferred-reduction operator-precedence driver over the explicit **parsing tape**, keyed on float `precedence` + `associativity` with one token of lookahead (not Pratt, whose hidden stack cannot express token-rewriting operators like the `X` case); pulls tokens on demand (so declarations land in the trie before later tokens lex); runs each token's native `constructor`; resolves names via the parser's own scope stack. Constructors for `:`,`=`,`:=`, `struct`,`array`,`mut`,`fn`, application, `+`/operators (monomorphizing in the constructor — `rational_number` literals mold to the sibling operand's type here). *Test:* parse `a = a + 1` and the `type`/`struct` defs into the LG shape the sketch shows. *The heart; mixed grind + care.*

**Phase 4 — Interpret tier.** `run`'s null-`bcode` body-walk (the tree-walk; interpretation is not a separate `fn` function); stack frames for locals (recursion works here); the ~30–40 endpoint natives (their `run`): binding, field/apply, arith/compare, `if`/`else`/`while`/`for`, `alloc`/`drop`/`&`/`@`, literal eval, `error`. *Mostly grind.*

**▶ Milestone 1 — `a = a + 1` runs interpreted.**

**Phase 5 ⚠ — Compile tier (Cranelift).** `fn.compile`: lower whole body to one IR graph — `types.rs` (LG→clif types/layout), `cfg.rs` (control flow→blocks, SSA via Variables), `lower.rs` (per-endpoint inst emission), finalize to `bcode` (`exec@`). `run` invokes `bcode` (fails if null). Stack calling convention; immediates baked. *Biggest chunk; your attention; Cranelift API churn — pin the version.*

**▶ Milestone 2 — the smoke test, wrapped in a `fn` (`main := fn () -> i32 ( … a = a + 1 … )`, since only functions compile), runs JIT-compiled; result diffed against the interpreter (oracle).** *(the flashy one)*

**Phase 6 — comptime / self-direction.** `compile()`+`run()` and `run { … }` execute inline during the pass; comptime runs without the I/O capability (reproducible). *Moderate.*

**▶ Milestone 3 — a program with a `struct`, a recursive `fn`, and `if`/`while` runs on both tiers, results identical.**

## Mutability & deopt (v1)

User code is structurally mutable (Option B); the seed is frozen. *Why this shape is, and why the eventual borrow checker makes it safe, lives in DESIGN ›Mutable code is compilable‹ — this section is only what the seed builds.* Code is data under the reader–writer rule: a compiled artifact is a **read reference** to code structure, and a structural write (a `constructor` splice at parse time, a `graph_mut` at run time) needs exclusive access, so it drops the readers — dropping a compiled reader *is* deopt. v1 builds the unsafe version (no borrow checker yet); three things to get right:

**Two automatic correctness floors (non-negotiable even though mutation is unsafe):**
1. **Name-index correctness.** Every structural edit maintains the trie *as part of the edit*. Needs: a back-edge on each *declaration* dyad to its trie entry (declaration-only metadata; anonymous nodes carry nothing); each trie leaf holds a **list of `id_context`** (`{identity, scope}`). Resolution filters that list by O(1) open-scope membership, and because shadowing is disallowed exactly one candidate is live (no nearest-ancestor tie-break). The index is **resident**: there is no scope-exit removal, an out-of-scope `id_context` is simply inert (filtered out), so the only removal is dropping a scope's `id_context`s when that scope is *structurally deleted*. A stale global index would silently mis-resolve for everyone, so this is never deferred to the programmer.
2. **`bcode` fail-closed.** `run` already fails on null `bcode`, so a *manual* deopt (null the `bcode`) always falls back to interpretation safely.

**One programmer-managed unsafe edge:**
3. **`bcode` freshness.** Structurally writing a compiled region without nulling its `bcode` makes the next `run` jump into stale machine code — UB, the use-after-free class. **OPEN DECISION (June 2026, pick before Phase 5's edit path):** v1 ships either **(a)** *no auto-deopt* — expose a manual deopt/`immut` primitive and trust the programmer (recommended; minimal seed; matches "mutation is unsafe like everything else"), or **(b)** *minimal auto-deopt* — a structural write conservatively nulls the enclosing fn's `bcode` (cheap; closes the common silent-UB footgun; a bit more seed).

**Cross-scope dangling references** (a use elsewhere that pointed into a deleted scope) are **not** found proactively in v1 (use-sets out of scope): because a `mut` region is interpreted, the dangling reference surfaces at the *next interpretation* as a resolution error. Proactive incremental detection is a post-v1 add on the same back-edges.

*Smoke-test gap:* the Milestone 1–3 programs don't exercise runtime structural mutation. Once Phase 3 lands, add a mutation smoke test — declare a name in a `mut (…)` region, `graph_mut`-delete a nested scope, then re-resolve.

## Risk concentration (the ⚠ list, in one place)

1. **Cyclic core-graph bootstrap** — wrong cycles/self-loop → baffling downstream failures. Build it first, assert against the sketch.
2. **Cranelift control-flow + SSA lowering** — the fiddliest code; the interpreter oracle is what makes it debuggable.
3. **Trie × one-pass interleaving** — declarations must reach the trie before later tokens lex; test by declaring-then-using in the same pass.

Everything else is volume, not difficulty — hand it to the LLM and review.
