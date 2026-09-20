// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The seed's hand-built core identities, one file each.
//!
//! Everything in Logos is an identity, but only the seed's *native* identities
//! are authored in Rust; identities created while a program runs are graph data,
//! never source files. This folder is that bounded native kernel: the node cell
//! ([`dyad`]) and name-resolution pairing ([`record`]) the substrate is
//! built from, plus each primitive (`logos`, `fn`, `i32`, `rational`, `=`, `+`).
//!
//! Each primitive file defines exactly one identity: its node, its spelling, and
//! its behaviour across the phases (parse construction, run native, compile
//! lowering). [`Core::build`] wires them into the graph. Structure — parse
//! parse_rank and associativity, and the layout values are read through (a
//! scalar width, operand arity and role names) — rides the graph as each
//! identity's shared-member record ([`meta`]; DESIGN ›A type's metadata is
//! shared by its values‹). Run behaviour rides the graph too (issue #44): each
//! native is a [`callable`] leaf the resolved nodes reference from their op
//! slots, so the interpreter consults no table — alternative run versions live
//! in versioned scopes, not in swapped HashMaps. Parse construction rides the
//! graph the same way: each identity's constructor is a callable leaf in its
//! record, minted from the build-time `metas` table, which drops before
//! parsing runs (bodies stay native until self-hosting). What stays
//! native-table-keyed is the Cranelift lowering (`lower`, until it re-keys
//! per backend identity).
//!
//! Deferred surface the sketch declares that the seed does not yet register —
//! tracked here so each gap is deliberate, not drift: the operators `^` (the
//! demo defines it in Logos, identities/power.logos) and `xor`; ranges as
//! first-class values, and `for`'s multi-variable and `gpu` forms; pointer
//! arithmetic; record parameters and returns (#115); operations over strings
//! (the `«…»` literal exists as an inert value); `mut` as a gate (#33); the
//! `hashtable` type and element-typed arrays (#47); error unions `(T | Error)`
//! (DESIGN ›Error handling‹); and the explicit `comptime` marker (the inferred
//! path — `-> type` calls resolving in the pass — exists). Each arrives with
//! the machinery it needs (layout, places, the borrow rule), not before.

use std::collections::HashMap;

use crate::compile::LowerTable;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ConstructFn, ParseError, FN_OUTPUT};
use crate::record::Record;
use crate::regex_trie::RegexTrie;
use crate::store::Store;

pub use numtype::NumType;

pub mod dyad;
pub mod read;
pub mod record;

mod and;
pub(crate) mod array;
mod assign;
mod binary;
#[path = "bool.rs"]
pub(crate) mod bool_mod;
pub(crate) mod callable;
mod colon;
mod comment;
mod convert;
pub(crate) mod declare;
pub(crate) mod drop_model;
#[path = "fn.rs"]
mod fn_mod;
#[path = "for.rs"]
mod for_mod;
pub(crate) mod fresh;
mod gate;
pub mod here;
mod hole;
#[path = "if.rs"]
mod if_mod;
pub mod import;
pub(crate) mod instance;
pub mod lex;
#[path = "logos.rs"]
mod logos_mod;
pub(crate) mod meta;
mod not;
pub(crate) mod numtype;
pub(crate) mod ops;
mod or;
mod paren;
pub(crate) mod pointer;
pub mod ran;
pub(crate) mod rational;
mod regex_mod;
#[path = "return.rs"]
mod return_mod;
pub(crate) mod run_body;
pub(crate) mod scope;
pub(crate) mod string;
pub mod tape;
pub mod this;
#[path = "while.rs"]
mod while_mod;

/// The core identities and the per-phase tables that drive them.
pub struct Core {
    /// The `logos := logos ?` self-loop, the one node whose type is itself.
    pub type_: DyadPtr,
    /// `scope`, the type of a scope node (the graph's spine). Each scope the parser
    /// opens is typed with this.
    pub scope: DyadPtr,
    /// `ran`, the type of an item that has run in the pass and carries its
    /// result (DESIGN ›Build and run are one self-directing pass‹, 13
    /// September 2026). Built by the parser, never spelled.
    pub ran_: DyadPtr,
    /// The scope every core identity is declared in; itself a `scope`-typed node.
    pub root_scope: DyadPtr,
    /// `fn`, the type whose values are functions.
    pub fn_type: DyadPtr,
    /// `i32`, the type of an integer variable/value (an alias for `numtypes[I32]`).
    pub i32_: DyadPtr,
    /// The numeric primitive type nodes, indexed by `NumType`. Unregistered logos
    /// (e.g. `f32`/`f64` before their phase) are null.
    pub numtypes: [DyadPtr; 10],
    /// `bool`, the type of a boolean value (a comparison result; an `if` condition).
    pub bool_: DyadPtr,
    /// `void`, the zero-sized unit logos: a `-> void` function yields unit (0 bits).
    pub void_: DyadPtr,
    /// `=` (assignment); a function.
    pub assign: DyadPtr,
    /// `convert`: the shared scalar numeric conversion, built from a `logos(value)`
    /// constructor and carrying its source/target logos per node.
    pub convert: DyadPtr,
    /// `+` (addition); resolves and stores its operand types per node.
    pub plus: DyadPtr,
    /// `-` (subtraction).
    pub minus: DyadPtr,
    /// `*` (multiplication).
    pub times: DyadPtr,
    /// `/` (division; total, saturating to MAX on a zero divisor).
    pub div_: DyadPtr,
    /// `%` (remainder; total, saturating to MAX on a zero divisor).
    pub rem_: DyadPtr,
    /// `<` (less-than comparison); its result is `bool`.
    pub lt: DyadPtr,
    /// `>` (greater-than comparison); its result is `bool`.
    pub gt: DyadPtr,
    /// `==` (equality comparison); its result is `bool`.
    pub eq: DyadPtr,
    /// `<=` (less-than-or-equal comparison); its result is `bool`.
    pub le: DyadPtr,
    /// `>=` (greater-than-or-equal comparison); its result is `bool`.
    pub ge: DyadPtr,
    /// `!=` (inequality comparison); its result is `bool`.
    pub ne: DyadPtr,
    /// `and` (short-circuiting logical conjunction); its result is `bool`.
    pub and_: DyadPtr,
    /// `or` (short-circuiting logical disjunction); its result is `bool`.
    pub or_: DyadPtr,
    /// `not` (logical negation); its result is `bool`.
    pub not_: DyadPtr,
    /// `if` (the value-producing conditional); a function.
    pub if_: DyadPtr,
    /// `while` (the loop statement); a function yielding unit.
    pub while_: DyadPtr,
    /// `for` (the counted-loop statement); a function yielding unit.
    pub for_: DyadPtr,
    /// `return` (the optional early yield); a function whose value is its operand.
    pub return_: DyadPtr,
    /// `declare`, the type of the declaration node `name := value` builds
    /// (`[name, declared, op]`); a statement yielding unit.
    pub declare_: DyadPtr,
    /// `compile`, the fn type's shared member (`f.compile()` — lower the body
    /// to machine code, install its `bcode`); a statement yielding unit.
    pub compile_: DyadPtr,
    /// `rational_number` (numeric literal carrier); a data type.
    pub rational: DyadPtr,
    /// `string` (the `«…»` text literal); inert in the seed, the comment substance.
    pub string_: DyadPtr,
    /// `regex`, the reader that turns a quote into a recognizer (#114): the
    /// node it places is what `:=` declares as a pattern spelling.
    pub regex_: DyadPtr,
    /// `comment` (the prose node a statement-level `#` builds); invisible to
    /// value flow.
    pub comment_: DyadPtr,
    /// `construct`, the record-construction statement a record-logos call builds.
    pub construct_: DyadPtr,
    /// `deref`, the dereference node postfix `@` builds.
    pub deref_: DyadPtr,
    /// `storeptr`, the store-through node `=` builds over a deref lhs.
    pub storeptr_: DyadPtr,
    /// `addr`, the address-of node prefix `&` builds — resolves its place's
    /// address at run/lower time (per-activation for a frame local).
    pub addr_: DyadPtr,
    /// `alloc`, the heap-allocation keyword — yields an owning pointer (issue #49).
    pub alloc_: DyadPtr,
    /// `own`, the ownership-move keyword — yields the pointer, empties the source.
    pub own_: DyadPtr,
    /// `drop`, the eager-teardown keyword — runs the place's destructor, empties it.
    pub drop_: DyadPtr,
    /// `free`, the allocator teardown `alloc` inserts as `defer free <place>`.
    pub free_: DyadPtr,
    /// `defer`, the scope-exit LIFO teardown holder (`defer <expr>`).
    pub defer_: DyadPtr,
    /// `pub`, the first gate identity (#33): a prefix word whose constructor
    /// fills the declare node's gate slot.
    pub pub_: DyadPtr,
    /// `import`, the one identity that loads a file (#58): its node is the
    /// reflectable trace of the load; running it re-yields the file's tail.
    pub import_: DyadPtr,
    /// `dyad`, the cell type: a value of it is the dyad view, `a:dyad`,
    /// whose `.type` and `.value` read the cell (#52, #70).
    pub dyad_: DyadPtr,
    /// `record`, the type of every name's record: the trie entry, a dyad
    /// whose value is the name's `dyad`, `scope`, `start`, `end`, `gate`.
    pub record_: DyadPtr,
    /// `:`, the record read.
    pub colon_: DyadPtr,
    /// `parsing_tape` and the tape's natives (#60).
    pub tape: tape::TapeIds,
    /// `this` in a parse body: the node being built, its slots read and
    /// written by field name (#133 slice 6).
    pub this: this::ThisIds,
    /// `lex`, the lexer as an identity, and its run leaf (#62).
    pub lex: lex::LexIds,
    pub run_body: run_body::RunBodyIds,
    /// `here` and `caller`, and the two `.scope` nodes (#123).
    pub here: here::HereIds,
    /// `index`, the passive node a `[i]` cell carries.
    pub index_: DyadPtr,
    /// `array` (of `dyad@`), the seed's first array form: a sequence's
    /// expression list lives behind one of these, never inline in the node.
    pub array_: DyadPtr,
    /// `callable`, the type whose values are the complete jump information
    /// (`[entry: @exec, convention]`); every exec leaf's logos (issue #44).
    pub callable_: DyadPtr,
    /// `convention`, the type whose values are calling-convention identities.
    pub convention_: DyadPtr,
    /// `seed-native`: the Rust-shim convention (`fn(&mut Runtime, node)`).
    pub conv_seed_native: DyadPtr,
    /// `container-i64`: the compiled-artifact convention (`i64` containers in a
    /// block, `(argv, argc)`; [`crate::run::MachineFn`]).
    pub conv_container: DyadPtr,
    /// `seed-parse`: the constructor convention (one [`ConstructFn`] signature
    /// for every identity).
    pub conv_seed_parse: DyadPtr,
    /// `(` — the opening paren/call token (parse-only).
    pub open_: DyadPtr,
    /// `)` — the closing paren token (parse-only).
    pub close_: DyadPtr,
    /// `[` — the opening square bracket (parse-only).
    pub open_sq_: DyadPtr,
    /// `]` — the closing square bracket (parse-only).
    pub close_sq_: DyadPtr,
    /// `,` — the one explicit separator (parse-only).
    pub sep_: DyadPtr,
    /// `left` and `right` — associativity's two values.
    pub left_: DyadPtr,
    pub right_: DyadPtr,
    /// The six slot words a type body may fill (`parse_rank`, `lex_rank`,
    /// `associativity`, `parse`, `run`, `instance`), in [`SLOT_NAMES`]
    /// order. Five fill the type's own value; `lex_rank` writes the record
    /// `:=` is filling (#122). Declared at the root today, a known
    /// divergence: the words belong to the type body alone (DESIGN ›The
    /// constructor is a field‹, 19 September 2026).
    pub slots: [DyadPtr; 6],
    /// `->` — the return-logos arrow (parse-only).
    pub arrow_: DyadPtr,
    /// `else` — the branch token `if`'s constructor consumes (parse-only).
    pub else_: DyadPtr,
    /// `in` — the loop-range token `for`'s constructor consumes (parse-only).
    pub in_: DyadPtr,
    /// `..` — the range token `for`'s constructor consumes (parse-only).
    pub dotdot_: DyadPtr,
    /// `.` — the field-access token (its constructor consumes `tape[-1]`).
    pub dot_: DyadPtr,
    /// `@` — the pointer token (postfix deref / pointer-logos prefix).
    pub at_: DyadPtr,
    /// `:=` — the declaration token (parse-only).
    pub declare_tok: DyadPtr,
    /// The concrete-op leaves (`add_i32`, `lt_f64`, `store_u8`, …), indexed for
    /// the parse-time resolver.
    pub ops: ops::OpLeaves,
    /// One compile version: each operation's Cranelift lowering rule. Parse
    /// behaviour lives on the constructor leaves the records carry (issue #30)
    /// and run behaviour on the callable leaves the nodes reference (issue
    /// #44); this one table retires into per-backend identities next.
    pub lower: LowerTable,
}

impl Core {
    /// Hand-build the core graph into `store`, registering spellings in `trie`.
    pub fn build(store: &mut Store, trie: &mut RegexTrie) -> Core {
        // Foundational logos first: others reference them.
        let type_ = logos_mod::register_root(store);
        let scope_ = scope::register(store, type_);
        let root_scope = store.alloc_raw(scope_, std::ptr::null_mut());
        // `record`: the type of every name's record (DESIGN ›The dyad's read
        // surface‹, 8 September 2026). Minted before any spelling is declared,
        // since each declaration allocates a record dyad of this type; its own
        // layout — five `@dyad` fields — is filled at the end of the build,
        // once `dyad` and `@` exist (`record::register_type`).
        let record_ = store.alloc_raw(type_, std::ptr::null_mut());
        // `string` is minted beside it, for the same reason: every record
        // carries its spelling as a string node (`a:name`, #120), so the type
        // must exist before the first declaration; its own record and the
        // `«…»` pattern are filled in by `string::register` below.
        let string_ = store.alloc_raw(type_, std::ptr::null_mut());
        let fn_type = fn_mod::register(store, type_);

        // Then the behaviour-bearing identities, via a shared build context.
        let mut cx = Cx {
            store,
            trie,
            type_,
            fn_type,
            root_scope,
            record_,
            string_,
            metas: HashMap::new(),
            lower: HashMap::new(),
        };
        // The numeric primitive types. Each self-describes its `NumType` (a tag in its
        // value slot); the shared lowering and interpreter read dispatch on the width.
        let mut numtypes: [DyadPtr; 10] = [std::ptr::null_mut(); 10];
        for &(spelling, nt) in &[
            ("i8", NumType::I8),
            ("i16", NumType::I16),
            ("i32", NumType::I32),
            ("i64", NumType::I64),
            ("u8", NumType::U8),
            ("u16", NumType::U16),
            ("u32", NumType::U32),
            ("u64", NumType::U64),
            ("f32", NumType::F32),
            ("f64", NumType::F64),
        ] {
            numtypes[nt as usize] = numtype::register_type(&mut cx, spelling, nt);
        }
        let i32_ = numtypes[NumType::I32 as usize];
        // `void`: the zero-sized unit logos (a `-> void` return). Self-describing via a
        // tag past the numeric range, so run/compile recognize it without a handle.
        let void = numtype::register_void(&mut cx);
        let bool_ = bool_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        // The text substance: `«…»` string literals and the comment nodes a
        // statement-level `#` builds over them. Registered before the operators,
        // whose records name their operands with string nodes.
        string::register(&mut cx);
        let comment_ = comment::register(&mut cx);
        let regex_ = regex_mod::register(&mut cx);
        // The callable machinery: the `callable`/`convention` type and the two
        // seed conventions. After `string` (convention names are string nodes),
        // before everything executable (exec leaves are callable values).
        let callables = callable::register(&mut cx);
        // The concrete machine operations: every (operation, machine type) pair
        // as a callable leaf, from one table-driven loop. The single-native
        // leaves (`and`, `or`, `convert`, …) are patched in by their files'
        // registrations below.
        let mut op_leaves = ops::register(&mut cx, &callables);
        // The array-of-dyad logos: a sequence's expression list rides behind one.
        let array_ = array::register(&mut cx);
        // The foundations allocated before the build context get their records
        // now: `logos`'s values are logos carrying records like its own (the
        // fixed point); a `scope`'s value is `[exprs, op]` — its expression
        // array and its sequence native (a scope IS an array; the list is
        // never inline in the node).
        let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::READER);
        // SAFETY: `type_`/`scope_` were allocated above with null value slots
        // nothing has read yet.
        unsafe {
            (*type_).value = record;
        }
        // The root's spelling (`logos`) and merged constructor are attached in
        // `logos_mod::register_syntax` below, once the punctuation it consumes
        // can register alongside it.
        let record = meta::operand_record(
            &mut cx,
            meta::TUPLE_TAG,
            meta::prec::INERT,
            Assoc::Left,
            &["exprs", "op"],
        );
        // SAFETY: `scope_` was minted above and nothing has read its value yet.
        unsafe {
            (*scope_).value = record;
        }
        let assign = assign::register(&mut cx);
        // The shared scalar numeric conversion (`i32(a)`, `f64(x)`, …). No spelling; the
        // parser builds conversion nodes from the `logos(value)` constructor surface.
        let (convert, convert_leaf) = convert::register(&mut cx, &callables);
        op_leaves.convert_ = convert_leaf;
        // The numeric operators. Each resolves its operand types at parse time and
        // stores it in the node's value slot; run/compile switch on it (see
        // `numtype`), so one identity per operator serves every numeric type.
        let binary::BinaryIds { plus, minus, times, div: div_, rem: rem_, lt, gt, eq, le, ge, ne } =
            binary::register_all(&mut cx);
        // The logical operators, over `bool` (their operands are comparisons/bools).
        let (and_, and_leaf) = and::register(&mut cx, &callables);
        op_leaves.and_ = and_leaf;
        let (or_, or_leaf) = or::register(&mut cx, &callables);
        op_leaves.or_ = or_leaf;
        let (not_, not_leaf) = not::register(&mut cx, &callables);
        op_leaves.not_ = not_leaf;
        let (if_, if_leaf, else_) = if_mod::register(&mut cx, &callables);
        op_leaves.if_ = if_leaf;
        let (while_, while_leaf) = while_mod::register(&mut cx, &callables);
        op_leaves.while_ = while_leaf;
        let (for_, for_leaf, in_, dotdot_) = for_mod::register(&mut cx, &callables);
        op_leaves.for_ = for_leaf;
        let arrow_ = fn_mod::register_syntax(&mut cx);
        // `compile`, the fn type's shared member (`f.compile()`); no spelling
        // — it resolves after `.` on an fn-typed value.
        let (compile_, compile_leaf) = fn_mod::register_compile(&mut cx, &callables);
        op_leaves.compile_ = compile_leaf;
        let (open_, close_) = paren::register(&mut cx);
        let (return_, return_leaf) = return_mod::register(&mut cx, &callables);
        op_leaves.return_ = return_leaf;
        // `:=`: the driver dispatches on the token's Construct and builds a
        // declare node — the declaration is graph structure, not parse vapor.
        let (declare_, declare_leaf, declare_tok) = declare::register(&mut cx, &callables);
        op_leaves.declare_ = declare_leaf;
        // `pub` (#33): the first gate — a prefix word over a declaration whose
        // constructor fills the declare node's gate slot.
        let pub_ = gate::register(&mut cx);
        // `import` (#58): the one identity that loads a file. The load runs in
        // the pass, once per run, over a DAG (ruled August 2026).
        let (import_, import_leaf) = import::register(&mut cx, &callables);
        op_leaves.import_ = import_leaf;
        // `dyad`, the cell type with its `dyad (type, value)` constructor
        // (#60), and `:`, the record read (#70).
        let dyad_ = dyad::register(&mut cx);
        let colon_ = colon::register(&mut cx);
        hole::register(&mut cx);
        let (sep_, left_, right_, slots) = logos_mod::register_syntax(&mut cx);
        // The two fresh-spelling patterns, ranked below every declared spelling.
        fresh::register(&mut cx);
        // Struct instances: the construction statement and the `.` field access.
        let (construct_, construct_leaf, dot_, index_, open_sq_, close_sq_) =
            instance::register(&mut cx, &callables);
        op_leaves.construct_ = construct_leaf;
        // Pointers: the `@`/`&` tokens and the deref/storeptr identities.
        let (deref_, storeptr_, addr_, deref_leaf, storeptr_leaf, addr_leaf, at_) =
            pointer::register(&mut cx, &callables);
        op_leaves.deref_ = deref_leaf;
        op_leaves.storeptr_ = storeptr_leaf;
        op_leaves.addr_ = addr_leaf;
        // The drop model (issue #49): `alloc`/`own`/`drop`/`free`/`defer`, their
        // run natives, and the shared teardown leaf (also every owning pointer's
        // destructor). Registered after pointers — its owning pointer is an `@T`.
        let dm = drop_model::register(&mut cx, &callables);
        op_leaves.alloc_ = dm.alloc_leaf;
        op_leaves.own_ = dm.own_leaf;
        op_leaves.drop_ = dm.drop_leaf;
        op_leaves.teardown_ = dm.teardown_leaf;
        op_leaves.defer_ = dm.defer_leaf;
        let (alloc_, own_, drop_, free_, defer_) =
            (dm.alloc_, dm.own_, dm.drop_, dm.free_, dm.defer_);
        // A multi-expression block is a `scope`-typed sequence node; its native
        // leaf and lowering are registered once the callable machinery exists.
        // The `record` type's own definition, last: its fields are `@dyad`
        // places, so it waits for `dyad` and `@` (DESIGN ›The dyad's read
        // surface‹, 8 September 2026).
        // `parsing_tape` and the four affordances as identities (#60).
        let tape = tape::register(&mut cx, &callables, scope_, array_, void);
        let this = this::register(&mut cx, &callables);
        // `lex`, the lexer as an identity (#62): its node runs the lexer and
        // yields a fragment `insert` splices.
        let lex = lex::register(&mut cx, &callables);
        // A type's held `run` body and the functions built from it (#133 slice 8).
        let run_body = run_body::register(&mut cx);
        // `here` and `caller` (#123): where a line is written, where it was
        // called from.
        let here = here::register(&mut cx, &callables);
        record::register_type(&mut cx, scope_, array_, dyad_, numtypes[NumType::F64 as usize]);
        op_leaves.scope_ = scope::register_exec(&mut cx, scope_, &callables);
        // An item that ran in the pass keeps its result beside it (R3, #88).
        let (ran_, ran_leaf) = ran::register(&mut cx, &callables);
        op_leaves.ran_ = ran_leaf;

        // Constructor slots (#30): the registration table was only ever the
        // collection point. Every identity's parse-time constructor moves onto
        // a callable leaf — the `seed-parse` convention, ONE entry signature
        // ([`ConstructFn`]) for every identity — installed in its record's
        // constructor slot; the table then drops here, before any parsing
        // runs. The fn-pointer→address cast is the licensed mint, exactly as
        // for the run natives (issue #44).
        let Cx { store, metas, lower, .. } = cx;
        // SAFETY: every key in `metas` is an identity whose registration built
        // its record, and each entry is minted from a `ConstructFn`.
        unsafe {
            for (&id, &construct) in &metas {
                let leaf = callable::mint(
                    store,
                    callables.callable,
                    construct as usize,
                    callables.seed_parse,
                );
                meta::install_constructor(id, leaf);
            }
        }
        drop(metas);
        Core {
            type_,
            scope: scope_,
            ran_,
            array_,
            root_scope,
            fn_type,
            i32_,
            numtypes,
            bool_,
            void_: void,
            assign,
            convert,
            plus,
            minus,
            times,
            div_,
            rem_,
            lt,
            gt,
            eq,
            le,
            ge,
            ne,
            and_,
            or_,
            not_,
            if_,
            while_,
            for_,
            return_,
            declare_,
            compile_,
            rational,
            string_,
            regex_,
            comment_,
            construct_,
            deref_,
            storeptr_,
            addr_,
            alloc_,
            own_,
            drop_,
            free_,
            defer_,
            pub_,
            import_,
            dyad_,
            record_,
            colon_,
            tape,
            this,
            lex,
            run_body,
            here,
            index_,
            callable_: callables.callable,
            convention_: callables.convention,
            conv_seed_native: callables.seed_native,
            conv_container: callables.container_i64,
            conv_seed_parse: callables.seed_parse,
            open_,
            close_,
            open_sq_,
            close_sq_,
            sep_,
            left_,
            right_,
            slots,
            arrow_,
            else_,
            in_,
            dotdot_,
            dot_,
            at_,
            declare_tok,
            ops: op_leaves,
            lower,
        }
    }

    /// The reading rule (DESIGN ›The dyad's read surface‹, 8 September 2026):
    /// a record operand yields the dyad it names; anything else passes
    /// through. One pointer compare per operand read.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        record::through(self.record_, p)
    }
}

/// The shared context each identity registers itself into: the store and name
/// index to build in, the foundational type handles it may reference, and the
/// per-phase tables it fills.
pub(crate) struct Cx<'a> {
    store: &'a mut Store,
    trie: &'a mut RegexTrie,
    type_: DyadPtr,
    fn_type: DyadPtr,
    root_scope: DyadPtr,
    /// The `record` type, minted first so every declaration can allocate its
    /// record dyad (`Cx::declare`).
    record_: DyadPtr,
    /// The `string` logos, minted before any declaration (its record and the
    /// `«…»` pattern filled in by `string::register`): every record's `name`
    /// is a string node, and an operand record's role names are too.
    string_: DyadPtr,
    metas: HashMap<DyadPtr, ConstructFn>,
    lower: LowerTable,
}

impl Cx<'_> {
    /// Declare `spelling` at the root scope as `id`: mint the name's record —
    /// a dyad of type `record`, one per declared name (DESIGN ›`mut` is a gate
    /// on the record‹) — index it, and return it.
    pub(crate) fn declare(&mut self, spelling: &str, id: DyadPtr) -> DyadPtr {
        let root = self.root_scope;
        self.declare_in(root, spelling, id)
    }

    /// Declare `spelling` in `scope` as `id`: the field names of a natively
    /// built record type live in that type's own scope, exactly as
    /// `Parser::parse_record` declares a user record's fields.
    pub(crate) fn declare_in(&mut self, scope: DyadPtr, spelling: &str, id: DyadPtr) -> DyadPtr {
        let name = string::build_text(self.store, self.string_, spelling.as_bytes());
        let record = Record::alloc(self.store, self.record_, Record::new(id, scope, name));
        self.trie.insert(spelling, record);
        record
    }
}

/// The one-signature infix constructor over a file's `build` fn: read the two
/// operands flanking the cursor from the tape (the model's `tape[-1]` and
/// `tape[+1]`, completed dyads at reduction) and build the operator node. With
/// no flanking operands — the driver invoking an extender that opened fresh —
/// the construct declines, and the operator shifts as a pending token (the
/// dangling-operator error path). Each operator file expands this over its own
/// `build`, keeping one constructor convention without hand-written wrappers.
macro_rules! infix_construct {
    ($build:path) => {{
        fn construct(
            p: &mut crate::parse::Parser,
            id: crate::dyad::DyadPtr,
            tape: &mut crate::parse::ParsingTape,
        ) -> Result<crate::parse::Constructed, crate::parse::ParseError> {
            let Some((lhs, rhs)) = p.binary_operands(tape)? else {
                return Ok(crate::parse::Constructed::Decline);
            };
            let types = p.types();
            let node = $build(p.store(), &types, id, lhs, rhs)?;
            tape.reduce_here(node);
            Ok(crate::parse::Constructed::Placed)
        }
        construct as crate::parse::ConstructFn
    }};
}
pub(crate) use infix_construct;

/// Run a top-level `defer` node's held teardown (issue #49) — the file driver's
/// program-exit drain of the top level's own bindings. Exposed so the binary
/// reaches the crate-private drop model without widening the whole module.
///
/// # Safety
/// `defer_node` must be a `defer` node from the store; `rt` its runtime.
pub unsafe fn run_deferred(
    rt: &mut crate::run::Runtime,
    defer_node: DyadPtr,
) -> Result<i64, crate::run::RunError> {
    rt.run(drop_model::deferred_inner_of(defer_node))
}

/// The two `dyad@` operands of a binary application node.
///
/// # Safety
/// `node.value` must point at an operand record of at least two `dyad@` fields,
/// as produced by [`build_binary`].
pub(crate) unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

/// A binary numeric operator operand's character, for logos resolution.
pub(crate) enum Operand {
    /// A value with a committed numeric type.
    Concrete(NumType),
    /// An uncommitted number literal (a `rational`), which molds to context.
    Literal,
    /// A pointer value, carrying its pointee type node. Pointer logos compare by
    /// pointee, never by node identity — they are created fresh per use.
    Pointer(DyadPtr),
    /// Not a number an operator can compute over (e.g. a `record`).
    NonNumeric,
}

/// Classify `node` as an operand of a numeric operator: its committed logos, an
/// uncommitted literal, or non-numeric.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn numtype_of(types: &Core, node: DyadPtr) -> Operand {
    let node = types.through(node);
    let logos = (*node).ty;
    // An item that ran in the pass yields what its expression yields.
    if logos == types.ran_ {
        return numtype_of(types, ran::expr_of(types, node));
    }
    if logos == types.rational {
        // A literal molds where it lands; a place of the type holds a
        // rational value, which no machine type takes silently.
        return if crate::dyad::is_place((*node).value) {
            Operand::NonNumeric
        } else {
            Operand::Literal
        };
    }
    // An arithmetic operator's result type is its left operand's: resolution
    // committed both operands to one type and stored the concrete op — not a
    // logos — in the op slot, so the type is read where it lives.
    if logos == types.plus
        || logos == types.minus
        || logos == types.times
        || logos == types.div_
        || logos == types.rem_
    {
        let lhs = *((*node).value as *const DyadPtr);
        return numtype_of(types, lhs);
    }
    // A comparison's or logical operator's result is `bool`, physically an
    // i32. An assignment yields nothing (`=` returns nothing, ruled 8
    // September 2026), so it falls through to non-numeric.
    if logos == types.lt
        || logos == types.gt
        || logos == types.le
        || logos == types.ge
        || logos == types.eq
        || logos == types.ne
        || logos == types.not_
        || logos == types.return_
    {
        return Operand::Concrete(NumType::I32);
    }
    // `and`/`or` over booleans is a boolean; over non-booleans it is a group,
    // no number at all (see [`and`]).
    if logos == types.and_ || logos == types.or_ {
        return if crate::parse::is_bool_result(types, node) {
            Operand::Concrete(NumType::I32)
        } else {
            Operand::NonNumeric
        };
    }
    // A tape's element read and the cell reads through it (#60, #61) yield a
    // cell's address: `t[k]`, `t[k]:dyad`, and `t[k]:dyad.type` are `@dyad`
    // values, so a write or an `insert` stores what they yield, never their
    // own address. `t.spelling[k]` (#121) yields a string node the same way,
    // the seed's form of a string value until strings are live: written into
    // a cell, the cell is that string.
    // `this.f` in a parse body (#133 slice 6) yields the node a slot holds,
    // the same way.
    if logos == types.tape.slot
        || logos == types.tape.slot_dyad
        || logos == types.tape.cell_type
        || logos == types.tape.spelling
        || logos == types.tape.slot_name
        || logos == types.this.slot
    {
        return Operand::Pointer(types.dyad_);
    }
    // `here`, `caller.scope`, and a scope's `.scope` (#123) yield a node's
    // address: an `@dyad` value, as `x:scope` is.
    if logos == types.here.here || logos == types.here.caller_scope || logos == types.here.scope_of
    {
        return Operand::Pointer(types.dyad_);
    }
    // A conversion's result is its target logos (stored at operand[2]).
    if logos == types.convert {
        return Operand::Concrete(numtype::of_type_node(numtype::stored_type(node)));
    }
    // A numeric variable/value: its type is one of the numeric type nodes.
    if types.numtypes.iter().any(|&t| !t.is_null() && t == logos) {
        return Operand::Concrete(numtype::of_type_node(logos));
    }
    // An else-less `if` yields unit, not a value (it has no false branch to
    // produce one); with both branches it takes the bare i32 default its
    // applications always had.
    if logos == types.if_ {
        if (*((*node).value as *const DyadPtr).add(2)).is_null() {
            return Operand::NonNumeric;
        }
        return Operand::Concrete(NumType::I32);
    }
    // A `while`/`for` loop, a record construction, a declaration, and a
    // `f.compile()` are statements yielding unit, never values. `drop`/`free`
    // (teardown) and `defer` (scope-exit holder) join them (issue #49).
    if logos == types.while_
        || logos == types.for_
        || logos == types.construct_
        || logos == types.declare_
        || logos == types.compile_
        || logos == types.drop_
        || logos == types.free_
        || logos == types.defer_
    {
        return Operand::NonNumeric;
    }
    // `alloc T v` and `own a` yield a pointer to the pointee they carry (issue
    // #49): `alloc` at operand 0, `own` at operand 1 — an owning pointer, but
    // owning-ness rides the *bound place's* logos, not this result classification.
    if logos == types.alloc_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr));
    }
    if logos == types.own_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr).add(1));
    }
    // A pointer-typed value (an `&x` literal, a pointer variable, or a pointer
    // field place): carries its pointee. Never arithmetic; passed and stored whole.
    if !logos.is_null() && numtype::is_pointer_type(logos) {
        return Operand::Pointer(numtype::pointee_of(logos));
    }
    // An address-of yields a pointer to its place's logos (the pointee it stores
    // at operand 1). Like deref/storeptr, its node type is its own identity, not
    // a pointer type, so numtype_of is the single classifier.
    if logos == types.addr_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr).add(1));
    }
    // A dereference yields its pointee's value; a store-through yields the stored
    // value, like `=`. Both must precede the generic fn-typed fallback below,
    // which would misread them as i32-returning calls.
    if logos == types.deref_ || logos == types.storeptr_ {
        let p = (*node).value as *const DyadPtr;
        let pointee = if logos == types.deref_ { *p.add(1) } else { *p.add(2) };
        if numtype::is_pointer_type(pointee) {
            return Operand::Pointer(numtype::pointee_of(pointee));
        }
        if is_numtype_node(types, pointee) {
            return Operand::Concrete(numtype::of_type_node(pointee));
        }
        return Operand::NonNumeric; // a record pointee reads only through `.field`
    }
    // A sequence's value is its trailing expression's. A literal tail takes the
    // bare-literal i32 default here rather than classifying as `Literal`: the
    // molding machinery commits a literal *node*, and this node is the sequence.
    if logos == types.scope {
        return match crate::parse::last_sequence_expr(node) {
            Some(last) => match numtype_of(types, last) {
                Operand::Literal => Operand::Concrete(NumType::I32),
                other => other,
            },
            None => Operand::NonNumeric,
        };
    }
    // A call: its result is the callee's return type. A self-call resolves through
    // the signature the declaration published onto its placeholder; only a
    // placeholder with no published signature (the value did not open with `fn`)
    // falls back to the i32 default. A void-returning callee yields no numeric
    // value (and its output has no NumType).
    // A type carrying a `code` is the call kind (#63; DESIGN ›Execution is
    // function application‹): a node of it yields what its code yields.
    let logos = if !logos.is_null()
        && meta::is_record_type(logos)
        && !meta::code_of(logos).is_null()
    {
        meta::code_of(logos)
    } else if !logos.is_null() && meta::is_record_type(logos) && !meta::run_body_of(logos).is_null()
    {
        // A node of a type whose `run` is a held body reads as the
        // function built for its field-type set, or as nothing until one
        // is (#133 slice 8).
        let spec = run_body::spec_of(node);
        if spec.is_null() {
            return Operand::NonNumeric;
        }
        spec
    } else {
        logos
    };
    if !logos.is_null() && (*logos).ty == types.fn_type {
        let fields = (*logos).value as *const DyadPtr;
        if !fields.is_null() {
            let out = *fields.add(FN_OUTPUT);
            if !out.is_null() && numtype::is_void_type(out) {
                return Operand::NonNumeric;
            }
            if !out.is_null() && numtype::is_pointer_type(out) {
                return Operand::Pointer(numtype::pointee_of(out));
            }
            // A call yielding a rational value (#133 slice 8, part 4).
            if !out.is_null() && out == types.rational {
                return Operand::NonNumeric;
            }
        }
        return Operand::Concrete(call_return_numtype(logos));
    }
    Operand::NonNumeric
}

/// The numeric return type of a fn node (its `FN_OUTPUT`), or `I32` when the callee
/// is an unbound placeholder with no published signature, or returns a non-numeric.
unsafe fn call_return_numtype(fn_node: DyadPtr) -> NumType {
    let fields = (*fn_node).value as *const DyadPtr;
    if fields.is_null() {
        return NumType::I32;
    }
    let out = *fields.add(FN_OUTPUT);
    if out.is_null() {
        NumType::I32
    } else {
        numtype::of_type_node(out)
    }
}

/// Resolve a binary numeric operator's operand types: commit any uncommitted
/// literal operand to it and return the committed operands with the resolved
/// [`NumType`], from which the family's builder picks its concrete-op leaf
/// (`add_i32`, `lt_f64`, …). Two different concrete type are a
/// [`ParseError::TypeMismatch`] (cross-type needs an explicit cast); a
/// non-numeric operand is [`ParseError::UnsupportedOperands`]; a literal that
/// has no exact value in the resolved type is
/// [`ParseError::UncomputableLiteral`].
///
/// # Safety
/// `lhs`/`rhs` are valid dyads from the store.
pub(crate) unsafe fn resolve_binary(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<([DyadPtr; 2], NumType), ParseError> {
    let a = numtype_of(types, lhs);
    let b = numtype_of(types, rhs);
    let nt = match (&a, &b) {
        // No pointer arithmetic: crossing addresses and numbers is deferred with
        // the rest of the pointer math (see `pointer`).
        (Operand::Pointer(_), _) | (_, Operand::Pointer(_)) => {
            return Err(ParseError::UnsupportedOperands)
        }
        (Operand::NonNumeric, _) | (_, Operand::NonNumeric) => {
            return Err(ParseError::UnsupportedOperands)
        }
        (Operand::Concrete(x), Operand::Concrete(y)) => {
            if x != y {
                return Err(ParseError::TypeMismatch);
            }
            *x
        }
        (Operand::Concrete(x), Operand::Literal) | (Operand::Literal, Operand::Concrete(x)) => *x,
        // Both uncommitted: default to i32 (arbitrary-precision rational is later work).
        (Operand::Literal, Operand::Literal) => NumType::I32,
    };
    let type_node = types.numtypes[nt as usize];
    let lhs = commit_if_literal(store, types, lhs, &a, type_node, nt)?;
    let rhs = commit_if_literal(store, types, rhs, &b, type_node, nt)?;
    Ok(([lhs, rhs], nt))
}

/// Commit an uncommitted literal operand to `nt` (a typed literal node holding the
/// molded bytes); non-literal operands pass through unchanged. A comptime
/// binding used as an operand is its record: the literal folds through it.
unsafe fn commit_if_literal(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    op: &Operand,
    type_node: DyadPtr,
    nt: NumType,
) -> Result<DyadPtr, ParseError> {
    if let Operand::Literal = op {
        // A comptime binding used here is its record; the literal folds
        // through it into a value of the committed type at this use.
        let bits =
            rational::mold_to(types.through(node), nt).ok_or(ParseError::UncomputableLiteral)?;
        let value = store.alloc_bytes(&bits.to_ne_bytes()[..nt.bytes()]);
        Ok(store.alloc_raw(type_node, value))
    } else {
        Ok(node)
    }
}

/// Whether `node` is one of the registered numeric type nodes (`i32`, `f64`, …). The
/// parser uses this to recognize a `logos(value)` conversion at a call site.
///
/// # Safety
/// `node` must be null or a valid dyad from the store (a use of a name is
/// its record, read through).
pub(crate) unsafe fn is_numtype_node(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    types.numtypes.iter().any(|&t| !t.is_null() && t == node)
}

/// Whether `node` is a type-value: a node classified by the `logos := logos ?`
/// root — a numeric type, the root itself, `bool`, a pointer or record type.
/// Logos identities are interned, so pointer identity *is* type identity, which
/// is what lets `==`/`!=` fold a comparison of two logos-values at parse time and
/// lets `.logos` yield a value comparable this way (roadmap #30).
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_type_value(types: &Core, node: DyadPtr) -> bool {
    type_identity_of(types, node).is_some()
}

/// The type identity `node` *is*, or `None` if it is not one in hand.
///
/// A node carrying `type` in its type slot and *not* an identity is a place
/// holding a type, which is an ordinary place (DESIGN ›A type is a comptime
/// value‹, 12 September 2026). It cannot answer a question about layout,
/// fields, or parse behavior, because what it holds is known only when the
/// program runs. The tag is the whole test: every place carries one and no
/// definition does (›GLOBAL_TAG‹). The four roots `type`, `fn`, `scope` and `record` are
/// minted with null values and back-filled with records in `Core::build`; what
/// keeps them type values is that a record is no place, not a null (#75).
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn type_identity_of(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    if read::read_kind(types, node) == read::Read::Identity {
        Some(node)
    } else {
        None
    }
}

/// The display spelling of a type-value (`i32`, `bool`, `type`, …). Numeric
/// type and `void` read their name from the record tag; the root and `bool`
/// are recognized by identity; other types-values (record type, pointers,
/// text) fall back to the generic `type` (the root's ruled spelling).
///
/// # Safety
/// `node` must satisfy [`is_type_value`].
unsafe fn type_name(types: &Core, node: DyadPtr) -> String {
    if node == types.type_ {
        return "type".to_string();
    }
    if node == types.bool_ {
        return "bool".to_string();
    }
    if node == types.record_ {
        return "record".to_string();
    }
    match meta::kind_of(node) {
        Some(t) if t <= NumType::F64 as u8 => NumType::from_tag(t).spelling().to_string(),
        Some(numtype::VOID_TAG) => "void".to_string(),
        _ => "type".to_string(),
    }
}

/// The place type and byte width a declaration's snapshot binding needs for a
/// runtime scalar `value`: a `Concrete` numeric (or `bool`) commits to its type
/// node at its width; a `Pointer` value (an `&x`, a pointer variable) becomes a
/// fresh `@pointee` place, 8 bytes wide, so `p := &x` gets real storage that
/// `p = &y` can rewire. The caller mints the place with this — frame-relative
/// inside a function, absolute at top level — and pairs it with
/// [`build_init`]. A bare rational (`x := 5`) stays a comptime binding
/// and never reaches here; a `fn`, logos, or unit value keeps its own binding.
///
/// # Safety
/// `value` must be a reduced dyad from the store whose [`numtype_of`] is
/// [`Operand::Concrete`] or [`Operand::Pointer`].
pub(crate) unsafe fn scalar_binding_type(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
) -> (DyadPtr, usize) {
    match numtype_of(types, value) {
        Operand::Concrete(nt) => (types.numtypes[nt as usize], nt.bytes()),
        Operand::Pointer(pointee) => {
            (pointer::make_pointer_type(store, types.type_, pointee), NumType::U64.bytes())
        }
        _ => unreachable!("scalar_binding_type needs a concrete or pointer value"),
    }
}

/// Build the initializing store of a declaration, `place = value`, the same
/// node `=` builds ([`assign::build`]): a scalar box takes the value at its
/// width, a node box its address. One builder for both, since the store node
/// does not care which (the reading rule decides at run, #82).
pub(crate) fn build_init(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    assign::build(store, types, types.assign, place, value)
}

/// Check a store's non-literal right side against the target's declared type —
/// the no-coercion rule (DESIGN ›two different concrete type do not silently
/// lower — there is no implicit coercion‹) applied to `=` and `p@ = …`. A
/// numeric target takes exactly its own width-kind (`bool` rides `I32` here, as
/// everywhere in the classifier); a pointer target takes a pointer to a matching
/// pointee; everything else — a cross-type value, a value into a pointer, a
/// pointer into a numeric, a unit-valued statement — is [`ParseError::TypeMismatch`].
/// An uncommitted literal never reaches this: the callers commit it to the
/// target's logos first (the typed slot), which is the one sanctioned crossing.
///
/// # Safety
/// `target_ty` must be a numeric or pointer type node and `rhs` a reduced dyad,
/// both from the store.
pub(crate) unsafe fn check_store_type(
    types: &Core,
    target_ty: DyadPtr,
    rhs: DyadPtr,
) -> Result<(), ParseError> {
    // What the target reads as decides what it takes (#82): an address whose
    // pointee matches, or a scalar of the same width and kind.
    let ok = match read::place_layout(types, target_ty) {
        Some((read::Read::Pointer(tp), _)) => {
            matches!(numtype_of(types, rhs), Operand::Pointer(p) if pointee_types_match(tp, p))
        }
        Some((read::Read::Scalar(nt), _)) => {
            matches!(numtype_of(types, rhs), Operand::Concrete(c) if c == nt)
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(ParseError::TypeMismatch)
    }
}

/// Whether two pointee type nodes denote the same type: the same node (numeric
/// and record type are interned singletons), or pointer type whose pointees
/// match recursively — pointer type nodes are minted per spelling, so `@@i32`
/// and `@@i32` are different nodes describing one logos.
///
/// # Safety
/// `a`/`b` must be type nodes from the store.
/// Plain pointer types are interned (#89), so this is `==` for them; what
/// it still bridges is an *owning* `@T` (fresh, its destructor its own)
/// against the plain `@T`, which name one pointee and match as types.
unsafe fn pointee_types_match(a: DyadPtr, b: DyadPtr) -> bool {
    a == b
        || (numtype::is_pointer_type(a)
            && numtype::is_pointer_type(b)
            && pointee_types_match(numtype::pointee_of(a), numtype::pointee_of(b)))
}

/// Render a run result for display: `bits` (the i64 the interpreter computes in)
/// interpreted through `node`'s static type — a float via its bit pattern, an
/// unsigned integer at its own width, a `bool` as `true`/`false` — so the CLI
/// prints `5.5` and `true`, not the raw bit container. Non-scalar and comptime
/// results fall back to the signed-decimal container, the plain default.
///
/// # Safety
/// `node` must be a valid dyad from the store (the parsed expression whose value
/// `bits` is).
pub unsafe fn display_value(types: &Core, node: DyadPtr, bits: i64) -> String {
    // A file or block is a scope whose value is its trailing expression; render
    // through that so the type-directed formatting below sees the actual value node
    // (a multi-line program ending in a type — or a float — not the scope wrapper).
    let node = types.through(trailing_expr(types, node));
    // A comparison / logical result is physically an i32; show its truth. A bool
    // stored into a variable reads back as its i32 0/1 (the seed has no distinct
    // bool storage), so only a direct bool-valued expression renders this way.
    if crate::parse::is_bool_result(types, node) {
        return if bits != 0 { "true" } else { "false" }.to_string();
    }
    // A rational value — a place of the type, a boxed literal, an operation
    // or call yielding one — carries the number's address (#133 slice 8,
    // part 4): show the number.
    if rational::is_rational_value(types, node) {
        let held = bits as usize as DyadPtr;
        if !held.is_null() && (*held).ty == types.rational {
            return rational::spell(held);
        }
    }
    // From here the reading rule says what the bits are (#82): the rest of
    // this function used to re-derive it with four tests of its own.
    match read::read_kind(types, node) {
        // A type is a first-class value; show its spelling, not the raw bit
        // container (roadmap #30) — a program ending in `i32` prints `i32`.
        read::Read::Identity => type_name(types, node),
        // A box shows what it holds: the container IS the held node's address
        // (DESIGN ›A type is a comptime value‹, 12 September 2026). A type in
        // it shows its spelling; an empty box is the hole it was declared as;
        // anything else in a `dyad ?` box shows as the dyad it is, exactly as a
        // view does — rendering the value behind it would mean running the
        // node, and display has no runtime. A bare parameter's container has
        // no declared type and shows its bits.
        read::Read::Container(_) => {
            let held = bits as usize as DyadPtr;
            let ty = (*node).ty;
            if ty.is_null() {
                bits.to_string()
            } else if held.is_null() {
                if ty == types.type_ { "type ?" } else { "dyad ?" }.to_string()
            } else if type_identity_of(types, held).is_some() {
                type_name(types, held)
            } else if (*held).ty == types.string_ {
                // A `string` place — a record's `name` (#120) — shows its
                // text: the one string read the seed has until strings are
                // live values.
                String::from_utf8_lossy(string::text(held)).into_owned()
            } else {
                "dyad".to_string()
            }
        }
        // A dyad view (#52) shows as the view it is, not its address bits.
        read::Read::Address => "dyad".to_string(),
        // A scalar formats at its type's width: a float via its bit pattern,
        // an unsigned integer at its own width. (A pointer is `U64`, and
        // every real address prints the same digits signed or unsigned.)
        read::Read::Scalar(nt) => format_scalar(nt, bits),
        read::Read::Pointer(_) => format_scalar(NumType::U64, bits),
        // An expression's result is typed by what it computes, which is
        // `numtype_of`'s question, not the reading rule's (#82's second half).
        _ => match numtype_of(types, node) {
            Operand::Concrete(nt) => format_scalar(nt, bits),
            _ => bits.to_string(),
        },
    }
}

/// Follow a scope to the trailing expression it evaluates to, so display formats the
/// real value node rather than the scope wrapper; nested scopes unwrap to the
/// innermost trailing expression. The run result `bits` is already that expression's
/// value (a scope's value *is* its trailing expression), so the two stay in step.
///
/// # Safety
/// `node` must be a valid dyad from the store.
unsafe fn trailing_expr(types: &Core, mut node: DyadPtr) -> DyadPtr {
    while !node.is_null() && (*node).ty == types.scope {
        match crate::parse::last_sequence_expr(node) {
            Some(inner) if inner != node => node = inner,
            _ => break,
        }
    }
    node
}

/// Format an `i64` bit container as its `NumType`: floats decoded from their
/// bits, unsigned integers read at their width, signed integers as-is (the
/// container is already sign-extended by [`numtype::read_scalar`]).
fn format_scalar(nt: NumType, bits: i64) -> String {
    use NumType::*;
    match nt {
        I8 => (bits as i8).to_string(),
        I16 => (bits as i16).to_string(),
        I32 => (bits as i32).to_string(),
        I64 => bits.to_string(),
        U8 => (bits as u8).to_string(),
        U16 => (bits as u16).to_string(),
        U32 => (bits as u32).to_string(),
        U64 => (bits as u64).to_string(),
        // `{:?}` always shows a decimal point (`5.0`, not `5`), so a float never
        // reads as an integer.
        F32 => format!("{:?}", f32::from_bits(bits as u32)),
        F64 => format!("{:?}", f64::from_bits(bits as u64)),
    }
}

/// Resolve a `for` range's operand types across its parts (start, end, optional
/// step), like [`resolve_binary`] over more operands: concrete type must all
/// match ([`ParseError::TypeMismatch`]), literals commit in place to the
/// resolved logos, all-literals default to i32, and a non-numeric part is
/// rejected. Returns the resolved numeric type node.
///
/// # Safety
/// `parts` must be reduced dyads from the store.
pub(crate) unsafe fn resolve_loop_parts(
    store: &mut Store,
    types: &Core,
    parts: &mut [DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let mut nt: Option<NumType> = None;
    for &p in parts.iter() {
        match numtype_of(types, p) {
            Operand::Concrete(c) => match nt {
                Some(n) if n != c => return Err(ParseError::TypeMismatch),
                _ => nt = Some(c),
            },
            Operand::Literal => {}
            Operand::Pointer(_) | Operand::NonNumeric => {
                return Err(ParseError::UnsupportedOperands)
            }
        }
    }
    let nt = nt.unwrap_or(NumType::I32);
    let logos = types.numtypes[nt as usize];
    for p in parts.iter_mut() {
        if let Operand::Literal = numtype_of(types, *p) {
            *p = commit_if_literal(store, types, *p, &Operand::Literal, logos, nt)?;
        }
    }
    Ok(logos)
}

/// Commit a rational literal node exactly to the numeric type `ty_node` — the
/// `logos literal` juxtaposition (`i32 32`, DESIGN ›an anonymous typed value is
/// written by juxtaposition‹). The result is a typed value with real storage.
///
/// # Safety
/// `lit` must be a rational literal from the store; `ty_node` a numeric type node.
pub(crate) unsafe fn commit_literal_to(
    store: &mut Store,
    types: &Core,
    lit: DyadPtr,
    ty_node: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let nt = numtype::of_type_node(ty_node);
    commit_if_literal(store, types, lit, &Operand::Literal, ty_node, nt)
}

/// Commit a call's uncommitted literal arguments to their parameters' declared
/// numeric type — the typed slot (DESIGN ›committing to a concrete type only when
/// it finally lands in a typed slot‹), so `f(3000000000)` is exact for an i64
/// parameter and `g(2.5)` reaches a float one. A non-fn callee, an unbound callee
/// (no published signature yet), an untyped parameter, or a non-literal argument
/// each pass through unchanged; extra arguments beyond the parameters are left for
/// the run/compile arity check. A literal with no exact value in its parameter's
/// type is [`ParseError::UncomputableLiteral`].
///
/// # Safety
/// `callee` and `args` must be valid dyads from the store.
pub(crate) unsafe fn commit_call_args(
    store: &mut Store,
    types: &Core,
    callee: DyadPtr,
    args: &mut [DyadPtr],
) -> Result<(), ParseError> {
    if (*callee).ty != types.fn_type {
        return Ok(());
    }
    let fields = (*callee).value as *const DyadPtr;
    if fields.is_null() {
        return Ok(());
    }
    let input = *fields.add(crate::parse::FN_INPUT);
    let params = array::items(meta::record_fields_of(input));
    for (i, arg) in args.iter_mut().enumerate() {
        let Some(&param) = params.get(i) else {
            break;
        };
        let pty = (*param).ty;
        // A bare `name` parameter accepts any dyad (DESIGN ›A function's
        // surface‹): nothing to check.
        if pty.is_null() {
            continue;
        }
        // What the parameter's place reads as says what an argument may be
        // (#82). A record or a code-carrying type has no whole read here and
        // is checked where that type is built (#115).
        match read::place_layout(types, pty) {
            // A pointer parameter takes only a pointer to the same pointee — a
            // committed literal here would be dereferenced as a wild address.
            // Pointees compare as types, not nodes (`pointee_types_match`):
            // pointer types are minted per spelling, so a `@@i32` parameter's
            // pointee and a `@@i32` argument's are two nodes for one type.
            Some((read::Read::Pointer(pp), _)) => match numtype_of(types, *arg) {
                Operand::Pointer(pointee) if pointee_types_match(pp, pointee) => {}
                _ => return Err(ParseError::TypeMismatch),
            },
            // A `dyad` parameter takes any node-valued argument — an identity,
            // a view, any declared box — the general box as a parameter
            // (DESIGN ›A type is a comptime value‹, 12 September 2026).
            Some((read::Read::Container(t), _)) if t == types.dyad_ => {
                let ok = match read::read_kind(types, *arg) {
                    read::Read::Identity | read::Read::Address => true,
                    read::Read::Container(c) => !c.is_null(),
                    _ => false,
                };
                if !ok {
                    return Err(ParseError::TypeMismatch);
                }
            }
            // A `type` parameter takes a type value, identity or `type` box,
            // and nothing else: a number into one would travel as an address.
            Some((read::Read::Container(t), _)) if t == types.type_ => {
                let ok = match read::read_kind(types, *arg) {
                    read::Read::Identity => true,
                    read::Read::Container(c) => c == types.type_,
                    _ => false,
                };
                if !ok {
                    return Err(ParseError::TypeMismatch);
                }
            }
            // A scalar parameter: a literal commits to it (the typed slot);
            // anything else must already be its type — the store a typed
            // place takes, no implicit coercion (DESIGN ›A function's surface‹:
            // "the caller's positional arguments are the parameter list's
            // holes, in order").
            Some((read::Read::Scalar(nt), _)) => {
                if (*types.through(*arg)).ty == types.rational {
                    *arg = commit_if_literal(store, types, *arg, &Operand::Literal, pty, nt)?;
                } else {
                    check_store_type(types, pty, *arg)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Commit a comptime-rational function body to its declared return type — the typed-slot
/// context (DESIGN ›Numeric literals are uncommitted until context classifies them‹). A `void` return,
/// or any non-concrete output, passes the body through; otherwise the body's tail value
/// positions are committed (see [`commit_tail`]).
///
/// # Safety
/// `body`/`output` are valid dyads from the store.
pub(crate) unsafe fn commit_fn_body(
    store: &mut Store,
    types: &Core,
    body: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // A `-> type` body's value is read back as a node address by
    // `eval_type_call`, so every tail leaf must be a type (#76).
    if output == types.type_ {
        check_type_tail(types, body)?;
        return Ok(body);
    }
    if !is_numtype_node(types, output) {
        return Ok(body);
    }
    commit_tail(store, types, body, output)
}

/// Walk the tail (value-producing) positions of `node`, applying `leaf` at each
/// leaf and writing back what it returns.
///
/// The tail positions are the leaves a function's value can come from: the node
/// itself, the operand of a `return`, both branches of an `if`, and a scope's
/// trailing non-comment expression — recursively, so `return (if …)`, nested
/// `if`s, and the like all reach their leaves. The constructs that yield unit
/// are enumerated here and refused, because the seed has no graph-driven
/// value-slot machinery yet (that arrives with self-hosting; #82). The branch
/// node is mutated in place, which is safe since it was just parsed and is not
/// yet aliased.
///
/// What a leaf must satisfy is the caller's business: [`commit_tail`] molds a
/// rational to a numeric output, [`check_type_tail`] demands a type.
///
/// # Safety
/// `node` is a valid dyad from the store.
unsafe fn walk_tail(
    types: &Core,
    node: DyadPtr,
    leaf: &mut impl FnMut(DyadPtr) -> Result<DyadPtr, ParseError>,
) -> Result<DyadPtr, ParseError> {
    // `return X`: X is the tail (the node's first slot, `[value, op]`).
    if (*node).ty == types.return_ {
        let ops = (*node).value as *mut DyadPtr;
        *ops = walk_tail(types, *ops, leaf)?;
        return Ok(node);
    }
    // `if (c) (then) else (else)`: both branches are tails (value `[cond, then, else]`).
    // An else-less `if` yields unit, so it cannot be a value function's tail.
    if (*node).ty == types.if_ {
        let ops = (*node).value as *mut DyadPtr;
        if (*ops.add(2)).is_null() {
            return Err(ParseError::MissingElse);
        }
        let then_c = walk_tail(types, *ops.add(1), leaf)?;
        let else_c = walk_tail(types, *ops.add(2), leaf)?;
        *ops.add(1) = then_c;
        *ops.add(2) = else_c;
        return Ok(node);
    }
    // A `while`/`for` loop, a construction, a declaration, an assignment, or
    // a `f.compile()` yields unit, so none of them can be a value function's
    // tail.
    if (*node).ty == types.while_
        || (*node).ty == types.for_
        || (*node).ty == types.construct_
        || (*node).ty == types.declare_
        || (*node).ty == types.assign
        || (*node).ty == types.storeptr_
        || (*node).ty == types.compile_
    {
        return Err(ParseError::StatementAsValue);
    }
    // A sequence: its trailing non-comment expression is the tail (trailing prose
    // is invisible to value flow). The expressions live behind the array node in
    // the sequence's first slot; the tail commits in place there.
    if (*node).ty == types.scope {
        let arr = scope::exprs_array(node);
        if !arr.is_null() {
            let (len, data) = array::parts(arr);
            let data = data as *mut DyadPtr;
            let mut i = len;
            while i > 0 {
                let cand = *data.add(i - 1);
                if !numtype::is_comment_type((*cand).ty) {
                    *data.add(i - 1) = walk_tail(types, cand, leaf)?;
                    break;
                }
                i -= 1;
            }
        }
        return Ok(node);
    }
    leaf(node)
}

/// Commit a comptime rational in tail position to `output`, a numeric type
/// node: a rational leaf molds to `output` (exact, else
/// [`ParseError::UncomputableLiteral`]) and everything else passes through.
/// The tail positions are [`walk_tail`]'s.
///
/// # Safety
/// `node`/`output` are valid dyads from the store; `output` is a numeric type node.
unsafe fn commit_tail(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    walk_tail(types, node, &mut |leaf| {
        if (*leaf).ty == types.rational && !crate::dyad::is_place((*leaf).value) {
            let nt = numtype::of_type_node(output);
            let bits = rational::mold_to(leaf, nt).ok_or(ParseError::UncomputableLiteral)?;
            let value = store.alloc_bytes(&bits.to_ne_bytes()[..nt.bytes()]);
            return Ok(store.alloc_raw(output, value));
        }
        // A pointer cannot be a numeric function's value (commit_tail runs only
        // for numeric outputs); rejecting here beats an invalid widen at the ABI.
        if let Operand::Pointer(_) = numtype_of(types, leaf) {
            return Err(ParseError::TypeMismatch);
        }
        // A rational value at run time has no machine form until it is
        // converted (#133 slice 8, part 4): crossing types is explicit.
        if rational::is_rational_value(types, leaf) {
            return Err(ParseError::TypeMismatch);
        }
        Ok(leaf)
    })
}

/// Require every tail leaf of a `-> type` function to actually be a type
/// (#76). `eval_type_call` reads a `-> type` call's result bits as a node
/// address, so a body that hands back a number hands back an address that was
/// never a node: `fn () -> type (5), f()` dereferenced 5 and took the process
/// down. The body is where that is knowable, so it is refused there.
///
/// # Safety
/// `node` is a valid dyad from the store.
unsafe fn check_type_tail(types: &Core, node: DyadPtr) -> Result<(), ParseError> {
    walk_tail(types, node, &mut |leaf| {
        if is_type_value(types, leaf) {
            Ok(leaf)
        } else {
            Err(ParseError::TypeMismatch)
        }
    })?;
    Ok(())
}

/// Build a scalar numeric conversion `target(operand)` — the `logos(value)` constructor
/// and the only cross-type path (DESIGN ›numeric conversion is the type constructor
/// consuming a value‹). A literal operand folds now, with `as` semantics, into a
/// `target`-typed value; a runtime operand of a *different* concrete type becomes a
/// [`convert`] node; the same concrete type passes through unchanged. Exactly one
/// numeric operand is required, else [`ParseError::BadCast`].
///
/// # Safety
/// `target` is a numeric type node; `args` are valid dyads from the store.
pub(crate) unsafe fn build_cast(
    store: &mut Store,
    types: &Core,
    target: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let [operand] = args else {
        return Err(ParseError::BadCast);
    };
    let operand = *operand;
    let to = numtype::of_type_node(target);
    match numtype_of(types, operand) {
        // A runtime value: a same-logos cast is a no-op, a different logos converts.
        Operand::Concrete(from) => {
            if from == to {
                Ok(operand)
            } else {
                let from_node = types.numtypes[from as usize];
                Ok(convert::build_convert(store, types, operand, from_node, target))
            }
        }
        // A literal: fold it into a `target`-typed value now, with `as` semantics.
        Operand::Literal => {
            let bits = rational::cast_to(types.through(operand), to)
                .ok_or(ParseError::UncomputableLiteral)?;
            let value = store.alloc_bytes(&bits.to_ne_bytes()[..to.bytes()]);
            Ok(store.alloc_raw(target, value))
        }
        // Pointer-to-integer casts are deferred with the rest of pointer math.
        Operand::Pointer(_) | Operand::NonNumeric => Err(ParseError::BadCast),
    }
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests;
