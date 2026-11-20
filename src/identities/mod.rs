// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The seed's hand-built core identities, one file each: the node cell (`dyad`),
//! the name binding (`binding`), and every primitive. `Core::build` wires them into
//! the graph; parse and run behaviour ride the graph as callable leaves, and only
//! the Cranelift lowering is still a native table.

use std::collections::HashMap;

use crate::binding::Binding;
use crate::compile::LowerTable;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ConstructFn, ParseError};
use crate::regex_trie::RegexTrie;
use crate::store::Store;

pub use numtype::NumType;

pub mod binding;
pub mod dyad;
pub mod read;

mod and;
pub(crate) mod array;
pub(crate) mod assign;
pub(crate) mod binary;
#[path = "bool.rs"]
pub(crate) mod bool_mod;
pub(crate) mod by_copy;
pub(crate) mod callable;
mod colon;
mod comment;
mod convert;
pub(crate) mod copy;
pub(crate) mod declare;
pub(crate) mod drop_model;
pub(crate) mod dyad_place;
pub mod error;
#[path = "fn.rs"]
mod fn_mod;
#[path = "for.rs"]
mod for_mod;
pub(crate) mod fresh;
mod gate;
pub(crate) mod group;
pub(crate) mod hashmap;
pub(crate) mod held_type;
pub mod here;
pub(crate) mod hole;
#[path = "if.rs"]
pub(crate) mod if_mod;
mod immediate;
pub mod import;
pub(crate) mod instance;
pub mod lex;
#[path = "logos.rs"]
mod logos_mod;
pub(crate) mod lossy;
pub(crate) mod meta;
pub(crate) mod mint;
mod not;
pub(crate) mod numtype;
pub(crate) mod ops;
mod or;
mod paren;
pub(crate) mod place;
pub(crate) mod pointer;
pub mod print;
pub(crate) mod rational;
mod regex_mod;
#[path = "return.rs"]
pub(crate) mod return_mod;
pub(crate) mod run_body;
pub(crate) mod scope;
pub(crate) mod string;
mod subset;
pub mod tape;
pub mod this;
pub(crate) mod waiting;
#[path = "while.rs"]
mod while_mod;

pub struct Core {
    /// The root type, the one node whose type is itself.
    pub type_: DyadPtr,
    pub scope: DyadPtr,
    /// The type of an item that ran in the pass and carries its result; never spelled.
    pub root_scope: DyadPtr,
    pub fn_type: DyadPtr,
    /// The same node as `numtypes[I32]`.
    pub i32_: DyadPtr,
    /// Indexed by `NumType`; an unregistered type is null.
    pub numtypes: [DyadPtr; 10],
    pub lossy: DyadPtr,
    pub bool_: DyadPtr,
    pub void_: DyadPtr,
    pub assign: DyadPtr,
    /// The scalar numeric conversion `T(value)`; it has no spelling of its own.
    pub convert: DyadPtr,
    pub plus: DyadPtr,
    /// Subtraction; negation is the other identity `-` spells.
    pub minus: DyadPtr,
    pub negate: DyadPtr,
    pub times: DyadPtr,
    pub div_: DyadPtr,
    pub rem_: DyadPtr,
    pub lt: DyadPtr,
    pub gt: DyadPtr,
    pub eq: DyadPtr,
    pub le: DyadPtr,
    pub ge: DyadPtr,
    pub ne: DyadPtr,
    pub and_: DyadPtr,
    pub or_: DyadPtr,
    pub not_: DyadPtr,
    pub subset: DyadPtr,
    pub if_: DyadPtr,
    pub while_: DyadPtr,
    pub for_: DyadPtr,
    pub return_: DyadPtr,
    pub declare_: DyadPtr,
    pub compile_: DyadPtr,
    /// `rational_number`, the numeric literal's type.
    pub rational: DyadPtr,
    /// The text type; inert in the seed.
    pub string_: DyadPtr,
    /// `«`, the token that builds a string; it reads its own extent.
    pub quote_: DyadPtr,
    /// The reader that turns a quote into the recognizer `:=` declares as a spelling.
    pub regex_: DyadPtr,
    /// The prose node a statement-level `#` builds; invisible to value flow.
    pub comment_: DyadPtr,
    /// `#`, the token that builds a comment; it reads its own extent.
    pub hash_: DyadPtr,
    pub construct_: DyadPtr,
    /// `p.x` over a record's storage: a place read through two bindings.
    pub field_: DyadPtr,
    pub deref_: DyadPtr,
    /// `&`, the type of the node its parse builds.
    pub amp_: DyadPtr,
    /// The address node no `&` was written for.
    pub addr_: DyadPtr,
    pub alloc_: DyadPtr,
    /// The gate word in a type position, `own @T ?`.
    pub own_: DyadPtr,
    pub move_: DyadPtr,
    pub free_: DyadPtr,
    pub defer_: DyadPtr,
    /// The type of an item of a scope's exit: a held name or an authored `defer`.
    pub exit_item: DyadPtr,
    /// The type of an `=`'s right side over a place that holds: built, then the displaced
    /// value freed.
    pub displace_: DyadPtr,
    /// The type of a call's argument the call holds: its value made or moved there, ended
    /// when the call returns.
    pub hold_: DyadPtr,
    /// The copy a place that holds its value takes of a value it would only borrow.
    pub copy_: DyadPtr,
    /// The gate word; its constructor fills the declare node's gate slot.
    pub pub_: DyadPtr,
    pub mut_: DyadPtr,
    pub immut_: DyadPtr,
    pub static_: DyadPtr,
    /// Its node is the trace of the load; running it re-yields the file's tail.
    pub import_: DyadPtr,
    /// The cell type.
    pub dyad_: DyadPtr,
    pub binding_: DyadPtr,
    /// `:`, the binding read.
    pub colon_: DyadPtr,
    /// `?`, the one unknown; also the read-veto entry its declarations put on a binding.
    pub unknown: DyadPtr,
    /// The type of the node a fresh spelling lexes to, holding the spelling.
    pub unknown_name: DyadPtr,
    pub tape: tape::TapeIds,
    pub hashmap: hashmap::HashmapIds,
    pub held_type: held_type::HeldTypeIds,
    pub this: this::ThisIds,
    pub place: place::PlaceIds,
    pub lex: lex::LexIds,
    pub print: print::PrintIds,
    pub error: error::ErrorIds,
    pub run_body: run_body::RunBodyIds,
    pub by_copy: by_copy::ByCopyIds,
    pub here: here::HereIds,
    pub dyad_place: dyad_place::DyadPlaceIds,
    pub waiting: waiting::WaitingIds,
    /// The identity of the cell a `[` lands, as `scope` is a `(`'s.
    pub square_brackets: DyadPtr,
    /// `array` of `dyad@`; a sequence's expression list lives behind one, never inline.
    pub array_: DyadPtr,
    pub callable_: DyadPtr,
    pub convention_: DyadPtr,
    /// The Rust-shim convention, `fn(&mut Runtime, node)`.
    pub conv_seed_native: DyadPtr,
    /// The compiled-artifact convention, `(argv, argc)` over i64 containers.
    pub conv_container: DyadPtr,
    /// The constructor convention, one `ConstructFn` signature for every identity.
    pub conv_seed_parse: DyadPtr,
    pub open_: DyadPtr,
    pub close_: DyadPtr,
    pub open_sq_: DyadPtr,
    pub close_sq_: DyadPtr,
    pub sep_: DyadPtr,
    /// Associativity's two values.
    pub left_: DyadPtr,
    pub right_: DyadPtr,
    pub arrow_: DyadPtr,
    pub else_: DyadPtr,
    pub in_: DyadPtr,
    pub of_: DyadPtr,
    pub dotdot_: DyadPtr,
    pub dot_: DyadPtr,
    pub at_: DyadPtr,
    pub declare_tok: DyadPtr,
    pub ops: ops::OpLeaves,
    pub lower: LowerTable,
}

impl Core {
    pub fn build(store: &mut Store, trie: &mut RegexTrie) -> Core {
        // Foundations first; everything below references them.
        let type_ = logos_mod::register_root(store);
        let scope_ = scope::register(store, type_);
        let root_scope = scope::mint(store, scope_, std::ptr::null_mut());
        // `binding` and `string` are minted before the first declaration, which needs
        // both; their own definitions are filled in below.
        let binding_ = store.alloc_leaf(type_);
        let string_ = store.alloc_leaf(type_);
        let fn_type = fn_mod::register(store, type_);

        let mut cx = Cx {
            store,
            trie,
            type_,
            fn_type,
            root_scope,
            binding_,
            string_,
            unknown: std::ptr::null_mut(),
            metas: HashMap::new(),
            extents: HashMap::new(),
            lower: HashMap::new(),
        };
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
        let lossy = lossy::register(&mut cx);
        let void = numtype::register_void(&mut cx);
        let bool_ = bool_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        // Before the operators: their records name operands with string nodes.
        let quote_ = string::register(&mut cx);
        let (comment_, hash_) = comment::register(&mut cx);
        let regex_ = regex_mod::register(&mut cx);
        // After `string`, before anything executable.
        let callables = callable::register(&mut cx);
        // The single-native leaves (`and`, `convert`, …) are patched in below.
        let mut op_leaves = ops::register(&mut cx, &callables);
        let array_ = array::register(&mut cx);
        let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::READER);
        // SAFETY: `type_` was minted above with a null value nothing has read.
        unsafe {
            dyad::set_head(type_, record);
        }
        // The root's spelling and constructor come with `logos_mod::register_syntax` below.
        let record = meta::operand_record(
            &mut cx,
            meta::TUPLE_TAG,
            meta::prec::READER,
            Assoc::Left,
            &["exprs", "op"],
        );
        // SAFETY: `scope_` was minted above and nothing has read its value yet; the record names
        // `exprs`.
        unsafe {
            dyad::set_head(scope_, record);
            meta::install_hands_on(scope_, &[], &["exprs"]);
        }
        let assign = assign::register(&mut cx);
        // No spelling: the parser builds conversions from the `T(value)` call surface.
        let (convert, convert_leaf) = convert::register(&mut cx, &callables);
        op_leaves.convert_ = convert_leaf;
        let binary::BinaryIds {
            plus,
            minus,
            negate,
            times,
            div: div_,
            rem: rem_,
            lt,
            gt,
            eq,
            le,
            ge,
            ne,
        } = binary::register_all(&mut cx);
        let (and_, and_leaf) = and::register(&mut cx, &callables);
        op_leaves.and_ = and_leaf;
        let (or_, or_leaf) = or::register(&mut cx, &callables);
        op_leaves.or_ = or_leaf;
        let (not_, not_leaf) = not::register(&mut cx, &callables);
        op_leaves.not_ = not_leaf;
        let (subset, subset_leaf) = subset::register(&mut cx, &callables);
        op_leaves.subset_ = subset_leaf;
        let (if_, if_leaf, else_) = if_mod::register(&mut cx, &callables);
        op_leaves.if_ = if_leaf;
        let (while_, while_leaf) = while_mod::register(&mut cx, &callables);
        op_leaves.while_ = while_leaf;
        let (for_, for_leaf, in_, dotdot_) = for_mod::register(&mut cx, &callables);
        op_leaves.for_ = for_leaf;
        let arrow_ = fn_mod::register_syntax(&mut cx);
        // No spelling: it resolves after `.` on an fn-typed value.
        let (compile_, compile_leaf) = fn_mod::register_compile(&mut cx, &callables);
        op_leaves.compile_ = compile_leaf;
        let (open_, close_) = paren::register(&mut cx);
        let (return_, return_leaf) = return_mod::register(&mut cx, &callables);
        op_leaves.return_ = return_leaf;
        let (declare_, declare_leaf, declare_tok) = declare::register(&mut cx, &callables);
        op_leaves.declare_ = declare_leaf;
        let (pub_, mut_, immut_, static_) = gate::register(&mut cx);
        let (import_, import_leaf) = import::register(&mut cx, &callables);
        op_leaves.import_ = import_leaf;
        let dyad_ = dyad::register(&mut cx);
        let colon_ = colon::register(&mut cx);
        let unknown = hole::register(&mut cx);
        cx.unknown = unknown;
        let (sep_, left_, right_) = logos_mod::register_syntax(&mut cx);
        let unknown_name = fresh::register(&mut cx);
        let instance::InstanceIds {
            construct: construct_,
            construct_leaf,
            field: field_,
            dot: dot_,
            square_brackets,
            open_sq: open_sq_,
            close_sq: close_sq_,
        } = instance::register(&mut cx, &callables);
        op_leaves.construct_ = construct_leaf;
        let pointer::PointerIds {
            deref: deref_,
            amp: amp_,
            addr: addr_,
            deref_leaf,
            addr_leaf,
            at: at_,
        } = pointer::register(&mut cx, &callables);
        op_leaves.deref_ = deref_leaf;
        op_leaves.addr_ = addr_leaf;
        // After pointers: the owning pointer is an `@T`.
        let dm = drop_model::register(&mut cx, &callables);
        op_leaves.alloc_ = dm.alloc_leaf;
        op_leaves.move_ = dm.move_leaf;
        op_leaves.free_ = dm.free_leaf;
        op_leaves.held_free_ = dm.held_free_leaf;
        op_leaves.instance_free_ = dm.instance_free_leaf;
        op_leaves.field_free_ = dm.field_free_leaf;
        op_leaves.value_free_ = dm.value_free_leaf;
        op_leaves.value_release_ = dm.value_release_leaf;
        op_leaves.teardown_ = dm.teardown_leaf;
        op_leaves.defer_ = dm.defer_leaf;
        op_leaves.displace_ = dm.displace_leaf;
        op_leaves.hold_ = dm.hold_leaf;
        let (alloc_, own_, move_, free_, defer_, exit_item, displace_, hold_, of_) = (
            dm.alloc_,
            dm.own_,
            dm.move_,
            dm.free_,
            dm.defer_,
            dm.exit_item,
            dm.displace_,
            dm.hold_,
            dm.of_,
        );
        let tape = tape::register(&mut cx, &callables, scope_, array_, void);
        let hashmap = hashmap::register(&mut cx, &callables, array_);
        let held_type = held_type::register(&mut cx, &callables);
        let this = this::register(&mut cx, &callables);
        let place = place::register(&mut cx, &callables);
        let lex = lex::register(&mut cx, &callables);
        let print = print::register(&mut cx, &callables);
        immediate::register(&mut cx);
        let error = error::register(&mut cx, &callables);
        let run_body = run_body::register(&mut cx);
        let by_copy = by_copy::register(&mut cx, &callables);
        let (copy_, copy_leaf) = copy::register(&mut cx, &callables);
        op_leaves.copy_ = copy_leaf;
        let here = here::register(&mut cx, &callables);
        let dyad_place = dyad_place::register(&mut cx, &callables);
        let waiting = waiting::register(&mut cx, &callables);
        // Last: the `binding` type's fields are `@dyad` places, so it waits for `dyad` and `@`.
        binding::register_type(
            &mut cx,
            scope_,
            array_,
            dyad_,
            numtypes[NumType::F64 as usize],
            numtypes[NumType::U64 as usize],
        );
        op_leaves.scope_ = scope::register_exec(&mut cx, scope_, &callables);

        // Every constructor and extent reader moves onto a callable leaf in its record; the
        // tables drop before any parsing runs.
        let Cx { store, metas, extents, lower, .. } = cx;
        // SAFETY: every key in `metas` and `extents` has its record built, and each entry is a
        // `ConstructFn` or an `ExtentFn`.
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
            for (&id, &extent) in &extents {
                let leaf = callable::mint(
                    store,
                    callables.callable,
                    extent as usize,
                    callables.seed_extent,
                );
                meta::install_extent(id, leaf);
            }
        }
        drop(metas);
        drop(extents);
        Core {
            type_,
            scope: scope_,
            array_,
            root_scope,
            fn_type,
            i32_,
            numtypes,
            lossy,
            bool_,
            void_: void,
            assign,
            convert,
            plus,
            minus,
            negate,
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
            subset,
            if_,
            while_,
            for_,
            return_,
            declare_,
            compile_,
            rational,
            string_,
            quote_,
            regex_,
            comment_,
            hash_,
            construct_,
            field_,
            deref_,
            amp_,
            addr_,
            alloc_,
            own_,
            move_,
            free_,
            defer_,
            exit_item,
            displace_,
            hold_,
            copy_,
            pub_,
            mut_,
            immut_,
            static_,
            import_,
            dyad_,
            binding_,
            colon_,
            unknown,
            unknown_name,
            tape,
            hashmap,
            held_type,
            this,
            place,
            lex,
            print,
            error,
            run_body,
            by_copy,
            here,
            dyad_place,
            waiting,
            square_brackets,
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
            arrow_,
            else_,
            in_,
            of_,
            dotdot_,
            dot_,
            at_,
            declare_tok,
            ops: op_leaves,
            lower,
        }
    }

    /// A binding yields the dyad it names; anything else passes through.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub unsafe fn through(&self, p: DyadPtr) -> DyadPtr {
        binding::through(self, p)
    }

    /// The frame a storage node's bytes lie in, with its offset: a binding laid out in
    /// the program frame or a call frame. `None` for a definition, a field's binding, a
    /// literal, a node.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn frame_of(&self, p: DyadPtr) -> Option<(binding::Frame, usize)> {
        if p.is_null() {
            return None;
        }
        if dyad::ty(p) == self.field_ {
            let (record, field) = instance::field_parts(p);
            let (frame, base) = self.frame_of(self.through(record))?;
            return Some((frame, base + Binding::read(field).offset));
        }
        if dyad::ty(p) != self.binding_ {
            return None;
        }
        let b = Binding::read(p);
        b.storage(self).map(|frame| (frame, b.offset))
    }

    /// Whether `p` is storage: bytes reached through a binding, never a value word.
    ///
    /// # Safety
    /// As `frame_of`.
    pub(crate) unsafe fn is_storage(&self, p: DyadPtr) -> bool {
        self.frame_of(self.through(p)).is_some()
    }

    /// The type a storage node's bytes are read by; `None` for what is not storage.
    ///
    /// # Safety
    /// As `frame_of`.
    pub(crate) unsafe fn storage_type(&self, p: DyadPtr) -> Option<DyadPtr> {
        let p = self.through(p);
        self.frame_of(p)?;
        Some(if dyad::ty(p) == self.field_ {
            hole::type_in(Binding::read(instance::field_parts(p).1).dyad)
        } else {
            Binding::read(p).dyad
        })
    }

    /// The field's binding when `p` is a `field` place over a binding's own record, the
    /// `x:f` read; `None` for any other node.
    ///
    /// # Safety
    /// As `frame_of`.
    pub(crate) unsafe fn binding_field_of(&self, p: DyadPtr) -> Option<DyadPtr> {
        if p.is_null() || dyad::ty(p) != self.field_ {
            return None;
        }
        let (record, field) = instance::field_parts(p);
        (self.type_of(record) == self.binding_).then_some(field)
    }

    /// The type of what a reduced operand denotes: its storage's declared type, or the
    /// type slot of the dyad it names or is.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub unsafe fn type_of(&self, p: DyadPtr) -> DyadPtr {
        let p = self.through(p);
        if p.is_null() {
            return p;
        }
        if let Some(t) = self.storage_type(p) {
            return t;
        }
        self.logos_of(p)
    }

    /// The type a node presents: a hole's or a field node's held type, else its type word.
    ///
    /// # Safety
    /// `p` must be a valid dyad from the store.
    pub(crate) unsafe fn logos_of(&self, p: DyadPtr) -> DyadPtr {
        if dyad::ty(p) == self.unknown {
            hole::type_in(p)
        } else {
            dyad::ty(p)
        }
    }

    /// Whether `p` is a `T ?` hole or a field node, a `?` holding its type.
    ///
    /// # Safety
    /// `p` must be null or a valid dyad from the store.
    pub(crate) unsafe fn is_hole(&self, p: DyadPtr) -> bool {
        hole::is_hole(self.unknown, p)
    }
}

/// The build context each identity registers itself into.
pub(crate) struct Cx<'a> {
    store: &'a mut Store,
    trie: &'a mut RegexTrie,
    type_: DyadPtr,
    fn_type: DyadPtr,
    root_scope: DyadPtr,
    /// Minted first: every declaration allocates a binding dyad of this type.
    binding_: DyadPtr,
    /// Minted before any declaration: every binding's name is a string node.
    string_: DyadPtr,
    /// `?`, the type of every hole and field node; null until `hole::register`.
    unknown: DyadPtr,
    metas: HashMap<DyadPtr, ConstructFn>,
    extents: HashMap<DyadPtr, crate::parse::ExtentFn>,
    lower: LowerTable,
}

impl Cx<'_> {
    /// Declare `spelling` at the root scope; returns the name's binding.
    pub(crate) fn declare(&mut self, spelling: &str, id: DyadPtr) -> DyadPtr {
        let root = self.root_scope;
        self.declare_in(root, spelling, id)
    }

    /// A native record type's field names live in the type's own scope. The
    /// spelling enters the index as a literal key, as a declared name does.
    pub(crate) fn declare_in(&mut self, scope: DyadPtr, spelling: &str, id: DyadPtr) -> DyadPtr {
        self.enter(scope, spelling, false, id)
    }

    /// Declare `pattern` at the root scope, entered as the recognizer it is.
    pub(crate) fn declare_pattern(&mut self, pattern: &str, id: DyadPtr) -> DyadPtr {
        let root = self.root_scope;
        self.enter(root, pattern, true, id)
    }

    fn enter(&mut self, scope: DyadPtr, spelling: &str, pattern: bool, id: DyadPtr) -> DyadPtr {
        let name = string::build_text(self.store, self.string_, spelling.as_bytes());
        let rec = Binding { pattern, ..Binding::new(id, scope, name) };
        let binding = Binding::alloc(self.store, self.binding_, rec);
        self.trie.insert(&rec.key(spelling), binding);
        binding
    }
}

/// The one infix constructor over a file's `build` fn: the two operands flanking the
/// cursor. With none (an extender that opened fresh) it declines, and the operator
/// shifts as a pending token.
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
            let node = crate::identities::group::apply(p.store(), &types, id, lhs, rhs, $build)?;
            // SAFETY: `node` was just built over reduced dyads.
            let node = unsafe { crate::identities::rational::slotted(p, node) };
            tape.reduce_here(node);
            Ok(crate::parse::Constructed::Placed)
        }
        construct as crate::parse::ConstructFn
    }};
}
pub(crate) use infix_construct;

/// The end of a scope every line of which completed: exposed so the binary can end the
/// REPL's session.
///
/// # Safety
/// `scope` must be a scope node from the store, its places in the frame `rt` runs.
pub unsafe fn run_scope_exit(
    rt: &mut crate::run::Runtime,
    scope: DyadPtr,
) -> Result<(), crate::run::RunError> {
    scope::run_exit(rt, scope, usize::MAX)
}

/// How many lines a scope's `dyads` hold: what a REPL line that fails gives back.
///
/// # Safety
/// `scope` must be a scope node from the store.
pub unsafe fn scope_lines(scope: DyadPtr) -> usize {
    scope::exprs_of(scope).map_or(0, <[_]>::len)
}

/// The scope keeps its first `kept` lines, so a failed REPL line leaves none in the session.
///
/// # Safety
/// `scope` must be a scope node from the store.
pub unsafe fn keep_scope_lines(scope: DyadPtr, kept: usize) {
    let lines = scope::exprs_array(scope);
    if !lines.is_null() {
        array::truncate(lines, kept);
    }
}

/// # Safety
/// `node.value` must point at an operand record of at least two `dyad@` fields.
pub(crate) unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// What a numeric operator's operand is, for type resolution.
pub(crate) enum Operand {
    Concrete(NumType),
    /// An uncommitted literal; it molds to context.
    Literal,
    /// Carries the pointee type; pointer types compare by pointee, never by node.
    Pointer(DyadPtr),
    NonNumeric,
}

/// An uncommitted literal molds to context; anything else is the type it gives back.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn operand_of(types: &Core, node: DyadPtr) -> Operand {
    if dyad::ty(types.through(node)) == types.rational {
        return Operand::Literal;
    }
    operand_of_type(types, read::output_type(types, node))
}

/// A value of type `t` as an operand: a rational holds a run-time rational, which no machine
/// type takes silently, a `bool` is no number, and a bracket parameter's container (`t` null)
/// is none either.
///
/// # Safety
/// `t` must be null or a type node from the store.
unsafe fn operand_of_type(types: &Core, t: DyadPtr) -> Operand {
    if t.is_null() || t == types.rational {
        Operand::NonNumeric
    } else if numtype::is_pointer_type(t) {
        Operand::Pointer(numtype::pointee_of(t))
    } else if is_numtype_node(types, t) {
        Operand::Concrete(numtype::of_type_node(t))
    } else {
        Operand::NonNumeric
    }
}

/// The number type two operands of `a` and `b` meet in: the one the other lies inside.
///
/// # Safety
/// `types` holds the number type nodes.
unsafe fn joint(types: &Core, a: NumType, b: NumType) -> Result<NumType, ParseError> {
    let node = |nt: NumType| types.numtypes[nt as usize];
    if subset::includes(types, node(a), node(b)) == Some(true) {
        Ok(b)
    } else if subset::includes(types, node(b), node(a)) == Some(true) {
        Ok(a)
    } else {
        Err(ParseError::TypeMismatch)
    }
}

/// Two number types meet in the one the other lies inside, the other operand converted; a
/// literal lands in the other operand's type, two literals in `i32`.
///
/// # Safety
/// `lhs`/`rhs` are valid dyads from the store.
pub(crate) unsafe fn resolve_binary(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<([DyadPtr; 2], NumType), ParseError> {
    let a = operand_of(types, lhs);
    let b = operand_of(types, rhs);
    let nt = match (&a, &b) {
        // A pointer step is built before this; any other pointer operand is refused.
        (Operand::Pointer(_), _) | (_, Operand::Pointer(_)) => {
            return Err(ParseError::UnsupportedOperands)
        }
        (Operand::NonNumeric, _) | (_, Operand::NonNumeric) => {
            return Err(ParseError::UnsupportedOperands)
        }
        (Operand::Concrete(x), Operand::Concrete(y)) => joint(types, *x, *y)?,
        (Operand::Concrete(x), Operand::Literal) | (Operand::Literal, Operand::Concrete(x)) => *x,
        (Operand::Literal, Operand::Literal) => NumType::I32,
    };
    let type_node = types.numtypes[nt as usize];
    let lhs = land(store, types, lhs, type_node)?;
    let rhs = land(store, types, rhs, type_node)?;
    Ok(([lhs, rhs], nt))
}

/// What lands in a place of type `ty`: the value itself where its type is `ty`, a plain number
/// converted exactly, a value whose type lies inside `ty` under the conversion node a written
/// `ty (value)` builds; anything else is refused where it is written. An `if` lands each arm,
/// so arms of two types meet in `ty`, and a block its last line. A type no place can have takes
/// the value as it stands. DESIGN ›A type's `parse` decides what converts into it; unwritten, a
/// value converts only into a type that holds it exactly‹, ›`if` reads its own right side‹.
///
/// # Safety
/// `value` must be a reduced dyad and `ty` null or a type node, both from the store.
pub(crate) unsafe fn land(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    land_with(store, types, value, ty, Landing::default())
}

/// A value of `rational_number` lands in the number type `ty` as [`land`] lands it: each plain
/// number it gives, an `if`'s arms and a block's last line included, converts, and a rational
/// known only when the program runs is refused. Any other value stands as written, for the
/// reader to take or refuse by its own type.
///
/// # Safety
/// As [`land`].
pub(crate) unsafe fn land_rational(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if read::output_type(types, value) != types.rational {
        return Ok(value);
    }
    land(store, types, value, ty)
}

/// As [`land`]; with `lossy`, what `ty lossy (value)` gives: a number of any number type, a plain
/// number or a rational value also converts into the number type `ty`, losing what `ty` cannot
/// hold.
///
/// # Safety
/// As [`land`].
pub(crate) unsafe fn land_as(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
    lossy: bool,
) -> Result<DyadPtr, ParseError> {
    land_with(store, types, value, ty, Landing { lossy, held: false })
}

/// As [`land`], into a place whose holder ends what it takes: a `dyad ?` place takes a value
/// that is no node in a new node of its own (›Holding is decided at the binding site,
/// parameters included‹ [[R61]]).
///
/// # Safety
/// As [`land`].
pub(crate) unsafe fn land_held(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    land_with(store, types, value, ty, Landing { lossy: false, held: true })
}

#[derive(Debug, Clone, Copy, Default)]
struct Landing {
    lossy: bool,
    held: bool,
}

/// # Safety
/// As [`land`].
unsafe fn land_with(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
    how: Landing,
) -> Result<DyadPtr, ParseError> {
    let Some((ty, (place, _))) =
        type_identity_of(types, ty).and_then(|t| Some((t, read::place_layout(types, t)?)))
    else {
        return Ok(value);
    };
    match read::value_type(types, value) {
        Ok(_) | Err(ParseError::ArmsDiffer) => {}
        Err(e) => return Err(e),
    }
    let landed =
        walk_tail(types, value, &mut |leaf| land_leaf(store, types, leaf, ty, place, how))?;
    read::value_type(types, landed)?;
    Ok(landed)
}

/// [`land`] at one position that gives the value.
///
/// # Safety
/// `value` must be a reduced dyad from the store; `ty` a type identity and `place` its layout.
unsafe fn land_leaf(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
    place: read::Read,
    how: Landing,
) -> Result<DyadPtr, ParseError> {
    if let Some(read) = dyad_place::read_through(store, types, value, ty, how.lossy) {
        return land_leaf(store, types, read, ty, place, how);
    }
    let lossy = how.lossy;
    // A comptime name used here is its binding; the literal folds through it.
    let plain = types.through(value);
    let plain_number = dyad::ty(plain) == types.rational;
    let from = read::output_type(types, value);
    let fits = match place {
        read::Read::Scalar(nt) if plain_number => {
            // `bool` shares `i32`'s machine form, so the type decides.
            if !is_numtype_node(types, ty) {
                return Err(ParseError::UncomputableLiteral);
            }
            let bits =
                if lossy { rational::cast_to(plain, nt) } else { rational::mold_to(plain, nt) }
                    .ok_or(ParseError::UncomputableLiteral)?;
            return Ok(store.alloc_blob(ty, &bits.to_ne_bytes()[..nt.bytes()]));
        }
        read::Read::Scalar(_) => {
            let number = is_numtype_node(types, from) || from == types.rational;
            let converts = subset::includes(types, from, ty) == Some(true)
                || lossy && number && is_numtype_node(types, ty);
            if from != ty && converts {
                return Ok(convert::build_convert(store, types, value, from, ty));
            }
            from == ty
        }
        read::Read::Pointer(pointee) => {
            numtype::is_pointer_type(from) && same_type(pointee, numtype::pointee_of(from))
        }
        read::Read::Container(t) if t == types.type_ => {
            if !assign::box_takes(types, t, value) {
                return Err(ParseError::BadDeclaredType);
            }
            true
        }
        read::Read::Container(t) if t == types.dyad_ => {
            return dyad_place::land(store, types, value, how.held);
        }
        read::Read::Container(t) => node_value_output(types, value) == Some(t),
        read::Read::Rational => plain_number || rational::is_rational_value(types, value),
        read::Read::Aggregate => from == ty,
        _ => false,
    };
    if fits {
        Ok(value)
    } else {
        Err(ParseError::TypeMismatch)
    }
}

/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_numtype_node(types: &Core, node: DyadPtr) -> bool {
    let node = types.through(node);
    types.numtypes.iter().any(|&t| !t.is_null() && t == node)
}

/// Type identities are interned, so pointer identity is type identity.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_type_value(types: &Core, node: DyadPtr) -> bool {
    type_identity_of(types, node).is_some()
}

/// The type identity `node` is, or `None`. A place holding a type is not one:
/// what it holds is known only when the program runs.
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

/// The type of nodes a Logos `parse` builds that `node` gives back, a hole's excepted: a hole
/// is no value.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn node_output(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    if types.is_hole(types.through(node)) {
        return None;
    }
    let t = read::output_type(types, node);
    meta::is_node_valued(t, types.fn_type).then_some(t)
}

/// The type `node` gives back when a value of it is its node's address, text included, a hole's
/// excepted.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn node_value_output(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    if types.is_hole(types.through(node)) {
        return None;
    }
    let t = read::output_type(types, node);
    meta::value_is_node(t, types.fn_type).then_some(t)
}

/// A `-> type` call's argument the pass can run now: a type, or a literal.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_comptime_arg(types: &Core, node: DyadPtr) -> bool {
    is_type_value(types, node) || matches!(operand_of(types, node), Operand::Literal)
}

/// The display spelling of a type value; a type with no spelling of its own shows as `type`.
///
/// # Safety
/// `node` must be a type identity.
unsafe fn type_name(types: &Core, node: DyadPtr) -> String {
    if node == types.type_ {
        return "type".to_string();
    }
    if node == types.bool_ {
        return "bool".to_string();
    }
    if node == types.binding_ {
        return "binding".to_string();
    }
    match meta::kind_of(node) {
        Some(t) if t <= NumType::F64 as u8 => NumType::from_tag(t).spelling().to_string(),
        Some(numtype::VOID_TAG) => "void".to_string(),
        _ => "type".to_string(),
    }
}

/// A type as an error message names it: a pointer type as it is written, `@` before its
/// pointee, which the echo leaves to the type's `print` slot (›A value is shown as the text its
/// type's `print` slot gives back‹ [[R305]]); any other type as [`type_name`] spells it.
///
/// # Safety
/// `node` must be a type identity.
pub(crate) unsafe fn message_type_name(types: &Core, node: DyadPtr) -> String {
    if numtype::is_pointer_type(node) {
        return format!("@{}", message_type_name(types, numtype::pointee_of(node)));
    }
    type_name(types, node)
}

/// The place type and byte width a value is copied into, by a declaration or `alloc`: the type
/// of what it gives at that type's width, a pointer as a plain `@pointee` place, the borrow,
/// whatever the value's own pointer type. `None` for a value no place holds whole, and for a
/// plain number, which stays the literal it is until a typed place molds it.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn copy_place(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
) -> Option<(DyadPtr, usize)> {
    if matches!(operand_of(types, value), Operand::Literal) {
        return None;
    }
    let t = read::output_type(types, value);
    let width = read::carry_of(types, t)?.width();
    if numtype::is_pointer_type(t) {
        return Some((
            pointer::make_pointer_type(store, types.type_, numtype::pointee_of(t)),
            width,
        ));
    }
    Some((t, width))
}

/// The initializing store of a declaration, the node `=` builds, past `=`'s gates: the
/// declaration writes its own storage once, whatever the name's gates.
pub(crate) fn build_init(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    assign::build_store(store, types, types.assign, place, value)
}

/// Same type, not same node: an owning `@T` and the plain `@T` name one pointee.
///
/// # Safety
/// `a`/`b` must be null or type nodes from the store.
pub(crate) unsafe fn same_type(a: DyadPtr, b: DyadPtr) -> bool {
    a == b
        || (numtype::is_pointer_type(a)
            && numtype::is_pointer_type(b)
            && same_type(numtype::pointee_of(a), numtype::pointee_of(b)))
}

/// Render a run result through `node`'s static type, so `5.5` and `true` print as such.
///
/// # Safety
/// `node` must be a valid dyad from the store, the expression whose value `bits` is.
pub unsafe fn display_value(types: &Core, node: DyadPtr, bits: i64) -> String {
    // A scope's value is its trailing expression; render that node.
    let node = types.through(trailing_expr(types, node));
    if read::output_type(types, node) == types.bool_ {
        return if bits != 0 { "true" } else { "false" }.to_string();
    }
    // A rational value's bits are the address of its sixteen bytes.
    if read::output_type(types, node) == types.rational {
        let p = bits as usize as *const u8;
        if !p.is_null() {
            return rational::spell_at(p);
        }
    }
    let kind = read::read_kind(types, node);
    // The bits are a node's address: what a box holds, or a value that is its node.
    if matches!(kind, read::Read::Container(_) | read::Read::Node)
        || read::output_type(types, node) == types.dyad_
        || node_value_output(types, node).is_some()
    {
        let ty = types.type_of(node);
        return if ty.is_null() { bits.to_string() } else { display_held(types, ty, bits) };
    }
    match kind {
        read::Read::Identity => type_name(types, node),
        read::Read::Address => "dyad".to_string(),
        read::Read::Scalar(nt) => format_scalar(nt, bits),
        read::Read::Pointer(_) => format_scalar(NumType::U64, bits),
        _ => match operand_of(types, node) {
            Operand::Concrete(nt) => format_scalar(nt, bits),
            _ => bits.to_string(),
        },
    }
}

/// What a box of type `ty` shows, its bits the held node's address: the node read through, a
/// number or a rational by its value. Rendering any other node would mean running it, and
/// display has no runtime, so it shows as `dyad`.
///
/// # Safety
/// `ty` must be a type node and `bits` 0 or the address of a live node.
unsafe fn display_held(types: &Core, ty: DyadPtr, bits: i64) -> String {
    let held = bits as usize as DyadPtr;
    if held.is_null() || held == types.unknown {
        return if ty == types.type_ { "type ?" } else { "dyad ?" }.to_string();
    }
    if type_identity_of(types, held).is_some() {
        return type_name(types, held);
    }
    let of = dyad::ty(held);
    // stand-in for #174: text shows as the text it holds.
    if of == types.string_ {
        return String::from_utf8_lossy(string::text(held)).into_owned();
    }
    let at = dyad::value(held);
    match read::place_layout(types, of) {
        Some((read::Read::Scalar(_), _)) if of == types.bool_ => {
            if numtype::read_scalar_nt(NumType::I32, at) != 0 { "true" } else { "false" }
                .to_string()
        }
        Some((read::Read::Scalar(nt), _)) => format_scalar(nt, numtype::read_scalar_nt(nt, at)),
        Some((read::Read::Rational, _)) => rational::spell_at(at),
        _ => "dyad".to_string(),
    }
}

/// The innermost trailing expression of a scope, whose value the run result is.
///
/// # Safety
/// `node` must be a valid dyad from the store.
unsafe fn trailing_expr(types: &Core, mut node: DyadPtr) -> DyadPtr {
    while !node.is_null() && dyad::ty(node) == types.scope {
        match crate::parse::last_sequence_expr(node) {
            Some(inner) if inner != node => node = inner,
            _ => break,
        }
    }
    node
}

/// The container is already sign-extended, so signed integers print as-is.
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
        // `{:?}` always shows a decimal point, so a float never reads as an integer.
        F32 => format!("{:?}", f32::from_bits(bits as u32)),
        F64 => format!("{:?}", f64::from_bits(bits as u64)),
    }
}

/// Like `resolve_binary` over a `for` range's parts; all-literal parts default to i32.
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
        match operand_of(types, p) {
            Operand::Concrete(c) => nt = Some(nt.map_or(Ok(c), |n| joint(types, n, c))?),
            Operand::Literal => {}
            Operand::Pointer(_) | Operand::NonNumeric => {
                return Err(ParseError::UnsupportedOperands)
            }
        }
    }
    let nt = nt.unwrap_or(NumType::I32);
    let logos = types.numtypes[nt as usize];
    for p in parts.iter_mut() {
        *p = land(store, types, *p, logos)?;
    }
    Ok(logos)
}

/// Land a call's arguments in their parameters' types, so `f(3000000000)` is exact for an `i64`
/// parameter. Extra arguments are left for the arity check.
///
/// # Safety
/// `callee` and `args` must be valid dyads from the store.
pub(crate) unsafe fn commit_call_args(
    store: &mut Store,
    types: &Core,
    callee: DyadPtr,
    args: &mut [DyadPtr],
) -> Result<(), ParseError> {
    let params = if dyad::ty(callee) == types.fn_type {
        let input = *(dyad::value(callee) as *const DyadPtr).add(crate::parse::FN_INPUT);
        array::items(meta::record_fields_of(input))
    } else {
        &[]
    };
    for (i, arg) in args.iter_mut().enumerate() {
        // A bracket parameter accepts any dyad.
        let pty = params.get(i).map_or(std::ptr::null_mut(), |&p| hole::type_in(p));
        if pty.is_null() {
            read::value_type(types, *arg)?;
        } else if drop_model::param_holds(types, callee, i) && !is_receiver(callee, i) {
            *arg = land_held(store, types, *arg, pty)?;
        } else {
            *arg = land(store, types, *arg, pty)?;
        }
    }
    Ok(())
}

/// Whether parameter `i` of the `fn` `callee` is the one no name declares: the value a `static`
/// function is reached through, which the call reads and writes where it stands.
///
/// # Safety
/// `callee` must be a `fn` node from the store.
unsafe fn is_receiver(callee: DyadPtr, i: usize) -> bool {
    let input = *(dyad::value(callee) as *const DyadPtr).add(crate::parse::FN_INPUT);
    array::items(meta::record_field_bindings_of(input))
        .get(i)
        .is_some_and(|&b| !b.is_null() && !binding::Binding::is_named(b))
}

/// Land every value a body gives, its last line's and each `return`'s, in the declared output
/// type.
///
/// # Safety
/// `body`/`output` are valid dyads from the store.
pub(crate) unsafe fn commit_fn_body(
    store: &mut Store,
    types: &Core,
    body: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if read::place_layout(types, output).is_none() {
        return Ok(body);
    }
    walk_tail(types, body, &mut |leaf| {
        // A line held for the run lands in the result when it runs.
        if dyad::ty(leaf) == types.waiting.line {
            waiting::set_into(types, leaf, output);
            return Ok(leaf);
        }
        refuse_statement(types, leaf)?;
        // An `out` already hands back its bytes.
        if dyad::ty(leaf) == types.by_copy.out {
            return Ok(leaf);
        }
        // The caller holds what the call gives.
        let landed = land_held(store, types, leaf, output)?;
        // A record result is handed back as the address of its bytes, which the call copies.
        if read::carry_of(types, output).and_then(read::Carry::bytes).is_some() {
            return Ok(by_copy::build_out(store, types, landed));
        }
        Ok(landed)
    })
}

/// Walk the value-producing positions of `node` (itself, a `return`'s operand, both
/// `if` branches, a scope's trailing expression) and write back what `leaf` returns.
///
/// # Safety
/// `node` is a valid dyad from the store.
unsafe fn walk_tail(
    types: &Core,
    node: DyadPtr,
    leaf: &mut impl FnMut(DyadPtr) -> Result<DyadPtr, ParseError>,
) -> Result<DyadPtr, ParseError> {
    if dyad::ty(node) == types.return_ {
        let ops = dyad::value(node) as *mut DyadPtr;
        *ops = walk_tail(types, *ops, leaf)?;
        return Ok(node);
    }
    // An `error` leaves the function, so it hands the tail nothing to check.
    if dyad::ty(node) == types.error.error {
        return Ok(node);
    }
    // An else-less `if` yields unit, so it cannot be a value function's tail.
    if dyad::ty(node) == types.if_ {
        let ops = dyad::value(node) as *mut DyadPtr;
        if (*ops.add(2)).is_null() {
            return Err(ParseError::MissingElse);
        }
        let then_c = walk_tail(types, *ops.add(1), leaf)?;
        let else_c = walk_tail(types, *ops.add(2), leaf)?;
        *ops.add(1) = then_c;
        *ops.add(2) = else_c;
        return Ok(node);
    }
    // Trailing prose is invisible to value flow, so the tail is the last non-comment expression.
    if dyad::ty(node) == types.scope {
        let arr = scope::exprs_array(node);
        if !arr.is_null() {
            let (len, data) = array::parts(arr);
            let data = data as *mut DyadPtr;
            let mut i = len;
            while i > 0 {
                let cand = *data.add(i - 1);
                if !numtype::is_comment_type(dyad::ty(cand)) {
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

/// A node that gives nothing is no value's tail.
///
/// # Safety
/// `node` is a valid dyad from the store.
unsafe fn refuse_statement(types: &Core, node: DyadPtr) -> Result<(), ParseError> {
    if read::output_type(types, node) == types.void_ {
        return Err(ParseError::StatementAsValue);
    }
    Ok(())
}

/// The written `T(value)`, or `T lossy (value)` with `lossy`, checked as an unwritten value
/// landing in `T` is. Over a line a check narrowed to `T`, the conversion stays a node, since
/// the line it places may be of another type (›A tape read checked against a number type reads
/// as that number‹ [[R229]]).
///
/// # Safety
/// `target` is a numeric type node; `args` are valid dyads from the store.
pub(crate) unsafe fn build_cast(
    store: &mut Store,
    types: &Core,
    target: DyadPtr,
    args: &[DyadPtr],
    lossy: bool,
) -> Result<DyadPtr, ParseError> {
    let [operand] = args else {
        return Err(ParseError::BadCast);
    };
    if tape::checked_number(types, *operand) == Some(target) {
        return Ok(convert::build_convert(store, types, *operand, target, target));
    }
    land_as(store, types, *operand, target, lossy)
}

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)] // a test reads the nodes it built a line above
mod tests;
