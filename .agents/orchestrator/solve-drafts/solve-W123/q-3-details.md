# q-3 details, for the orchestrator (not for Thobias)

All probes: dev fbaf028 (no seed change since 5dbf3b2), `target/release/logos`, interpreted.

## His point 1: does every node's `parse` run? No.

- DESIGN ›A `tape[k]` read past the lexed frontier lexes on demand for every constructor‹: "A cell the reader leaves on the tape is built in its normal turn, at the loop's next step." A node another constructor removes never gets a turn.
- Probe: `h := type (share parse_rank = *.parse_rank - 1, share parse = ( print «h ran», tape.is_constructed[0] = true )), g := type (share parse_rank = *.parse_rank + 1, share parse = ( tape.remove(1), tape.is_constructed[0] = true )), g h, 5` → `5`, exit 0, no "h ran". (With h at the default rank, h runs at discovery and prints "h ran" first; the lower rank is needed for the probe.)
- Also in ordinary code: `array i32 [1, 2]` (array takes `i32`, identities/array.logos `t := tape[1]` … `tape.remove(1)`); `zz := 5` (`:=` takes `zz`).

## His point 2: can the flag be set automatically after each parse? Not without losing hand-on.

- DESIGN ›The constructor sets `tape.is_constructed[0] = true` itself; a tape write never sets it‹ (Thobias, 19 September 2026): "A constructor that finishes its cell sets the flag; one that hands the cell on leaves it false." Why: "a constructor may leave an unfinished value (the array chooser places the mint, whose own `parse` the driver still runs), so a write cannot mean "done"; only the constructor knows."
- DESIGN ›A constructor that finds nothing to consume sets its flag and stands as itself‹: "Driver rule: flag true, done. False and the cell holds another identity: that one's turn. False and the same identity: checked error (neither finished nor handed on)."
- His words (Q88): "is_constructed(0) can be sat to true automatcically after each parse has runned".
- Probe, hand-on: `h := type (share parse_rank = *.parse_rank + 1, share parse = ( tape[0] = i32 )), x := h 5, print «{x}»` → `5`, exit 0.
- Probe, finished: the same with `, tape.is_constructed[0] = true` after `tape[0] = i32` → `error: expected one expression, found more`, exit 1.
- Probe, neither: `h := type (share parse = ( print «ran» )), h, 5` → `ran`, then "error: the constructor of `h` edited the tape but left its own cell unconstructed: place what it built and set `tape.is_constructed[0] = true`, hand the cell to another identity, remove it, or set the flag on the untouched cell to stand as itself", exit 1. (The message says "edited" though this parse edited nothing; run_ctor does not track edits for a Logos constructor.)
- The array chooser hands on: identities/array.logos, last branch `tape[0] = mint, tape.remove(1)` with no flag write.

## His point 3: is the flag read only inside a parse? No.

- DESIGN ›`tape` is a word the `parse` slot declares into its body‹: "Bound per run to the tape centred on the appearance; ordinary name resolution, undeclared elsewhere."
- The driver reads every node's flag (src/parse/driver.rs construct_segment `if c.constructed { continue; }`, run_ctor's outcome rule).
- DESIGN ›What `lex` returns: a `parsing_tape` fragment‹: "the cells the lexer would have put on the frontier, with their flags and their spellings"; a fragment is a value usable anywhere. The seed refuses that read outside a parse (P133).

## Point 4: S41 needs none of the three

S41 says only which node a constructor may flag: a removed node needs no flag, and a node left on the tape gets its own turn.

## Rename scope, the counts behind Q3

- `:name`: DESIGN.md 13, tests/cli.rs 13, language_sketch.logos 2; seed: binding field `("name", cx.string_)` src/identities/binding.rs:307, `:name` read src/parse/fields.rs:1085, `tape[k]:name` run_slot_name src/identities/tape.rs:680.
- `unknown_name`: DESIGN.md 3 (the Q82 Ruled line and History).
- Message "unknown name": DESIGN.md 3; src and tests 24.
- DESIGN.md name/names/named (all senses): 541; `###` headings with the word: 31 (ordinary English among them: ›Versions are named vX.Y.Z‹, ›Mandatory named arguments are rejected‹, ›Slots are named for the moment they run‹, "an error names a category").
- Seed Rust: 751 uses in 48 files; 46 string literals.
- Other docs (README, AGENTS and the like, outside .agents/): 104. DESIGN_HISTORY.md: 149 (past wordings). docs/v0.0.3 and docs/v0.0.4: 8 each in prose, frozen by the docs guard.
- Clash: "type word" (the first 8 bytes of a node, its type) 23 times in DESIGN.md, ›A dyad is a type and a value: one block, the type word first, and its identity is its address‹.

## Pending record

S56 is to be recorded chosen with his Q88 words ("a but lets call it unkown_word instead …"), its text amended from unknown_name to unknown_word, once Q3 settles the rename's scope.
