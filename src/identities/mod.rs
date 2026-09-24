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
use crate::parse::{Assoc, ConstructFn, ParseError, FN_OUTPUT};
use crate::regex_trie::RegexTrie;
use crate::store::Store;

pub use numtype::NumType;

pub mod binding;
pub mod dyad;
pub mod read;

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
pub mod error;
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
pub mod print;
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

pub struct Core {
    /// The root type, the one node whose type is itself.
    pub type_: DyadPtr,
    pub scope: DyadPtr,
    /// The type of an item that ran in the pass and carries its result; never spelled.
    pub ran_: DyadPtr,
    pub root_scope: DyadPtr,
    pub fn_type: DyadPtr,
    /// The same node as `numtypes[I32]`.
    pub i32_: DyadPtr,
    /// Indexed by `NumType`; an unregistered type is null.
    pub numtypes: [DyadPtr; 10],
    pub bool_: DyadPtr,
    pub void_: DyadPtr,
    pub assign: DyadPtr,
    /// The scalar numeric conversion `T(value)`; it has no spelling of its own.
    pub convert: DyadPtr,
    pub plus: DyadPtr,
    pub minus: DyadPtr,
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
    pub if_: DyadPtr,
    pub while_: DyadPtr,
    pub for_: DyadPtr,
    pub return_: DyadPtr,
    pub declare_: DyadPtr,
    pub compile_: DyadPtr,
    /// `rational_number`, the numeric literal's type.
    pub rational: DyadPtr,
    /// The `«…»` literal; inert in the seed.
    pub string_: DyadPtr,
    /// The reader that turns a quote into the recognizer `:=` declares as a spelling.
    pub regex_: DyadPtr,
    /// The prose node a statement-level `#` builds; invisible to value flow.
    pub comment_: DyadPtr,
    pub construct_: DyadPtr,
    pub deref_: DyadPtr,
    /// The store-through node `=` builds over a deref left side.
    pub storeptr_: DyadPtr,
    pub addr_: DyadPtr,
    pub alloc_: DyadPtr,
    pub own_: DyadPtr,
    pub drop_: DyadPtr,
    /// The teardown `alloc` inserts as `defer free <place>`.
    pub free_: DyadPtr,
    pub defer_: DyadPtr,
    /// The gate word; its constructor fills the declare node's gate slot.
    pub pub_: DyadPtr,
    pub mut_: DyadPtr,
    pub immut_: DyadPtr,
    pub shared_: DyadPtr,
    /// Its node is the trace of the load; running it re-yields the file's tail.
    pub import_: DyadPtr,
    /// The cell type.
    pub dyad_: DyadPtr,
    pub binding_: DyadPtr,
    /// `:`, the binding read.
    pub colon_: DyadPtr,
    pub tape: tape::TapeIds,
    pub this: this::ThisIds,
    pub lex: lex::LexIds,
    pub print: print::PrintIds,
    pub error: error::ErrorIds,
    pub run_body: run_body::RunBodyIds,
    pub here: here::HereIds,
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
        let root_scope = store.alloc_raw(scope_, std::ptr::null_mut());
        // `binding` and `string` are minted before the first declaration, which needs
        // both; their own definitions are filled in below.
        let binding_ = store.alloc_raw(type_, std::ptr::null_mut());
        let string_ = store.alloc_raw(type_, std::ptr::null_mut());
        let fn_type = fn_mod::register(store, type_);

        let mut cx = Cx {
            store,
            trie,
            type_,
            fn_type,
            root_scope,
            binding_,
            string_,
            metas: HashMap::new(),
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
        let void = numtype::register_void(&mut cx);
        let bool_ = bool_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        // Before the operators: their records name operands with string nodes.
        string::register(&mut cx);
        let comment_ = comment::register(&mut cx);
        let regex_ = regex_mod::register(&mut cx);
        // After `string`, before anything executable.
        let callables = callable::register(&mut cx);
        // The single-native leaves (`and`, `convert`, …) are patched in below.
        let mut op_leaves = ops::register(&mut cx, &callables);
        let array_ = array::register(&mut cx);
        let record = meta::record(cx.store, meta::TYPEREC_TAG, meta::prec::READER);
        // SAFETY: `type_` was minted above with a null value nothing has read.
        unsafe {
            (*type_).value = record;
        }
        // The root's spelling and constructor come with `logos_mod::register_syntax` below.
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
        // No spelling: the parser builds conversions from the `T(value)` call surface.
        let (convert, convert_leaf) = convert::register(&mut cx, &callables);
        op_leaves.convert_ = convert_leaf;
        let binary::BinaryIds { plus, minus, times, div: div_, rem: rem_, lt, gt, eq, le, ge, ne } =
            binary::register_all(&mut cx);
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
        // No spelling: it resolves after `.` on an fn-typed value.
        let (compile_, compile_leaf) = fn_mod::register_compile(&mut cx, &callables);
        op_leaves.compile_ = compile_leaf;
        let (open_, close_) = paren::register(&mut cx);
        let (return_, return_leaf) = return_mod::register(&mut cx, &callables);
        op_leaves.return_ = return_leaf;
        let (declare_, declare_leaf, declare_tok) = declare::register(&mut cx, &callables);
        op_leaves.declare_ = declare_leaf;
        let (pub_, mut_, immut_, shared_) = gate::register(&mut cx);
        let (import_, import_leaf) = import::register(&mut cx, &callables);
        op_leaves.import_ = import_leaf;
        let dyad_ = dyad::register(&mut cx);
        let colon_ = colon::register(&mut cx);
        hole::register(&mut cx);
        let (sep_, left_, right_) = logos_mod::register_syntax(&mut cx);
        fresh::register(&mut cx);
        let (construct_, construct_leaf, dot_, square_brackets, open_sq_, close_sq_) =
            instance::register(&mut cx, &callables);
        op_leaves.construct_ = construct_leaf;
        let (deref_, storeptr_, addr_, deref_leaf, storeptr_leaf, addr_leaf, at_) =
            pointer::register(&mut cx, &callables);
        op_leaves.deref_ = deref_leaf;
        op_leaves.storeptr_ = storeptr_leaf;
        op_leaves.addr_ = addr_leaf;
        // After pointers: the owning pointer is an `@T`.
        let dm = drop_model::register(&mut cx, &callables);
        op_leaves.alloc_ = dm.alloc_leaf;
        op_leaves.own_ = dm.own_leaf;
        op_leaves.drop_ = dm.drop_leaf;
        op_leaves.teardown_ = dm.teardown_leaf;
        op_leaves.defer_ = dm.defer_leaf;
        let (alloc_, own_, drop_, free_, defer_, of_) =
            (dm.alloc_, dm.own_, dm.drop_, dm.free_, dm.defer_, dm.of_);
        let tape = tape::register(&mut cx, &callables, scope_, array_, void);
        let this = this::register(&mut cx, &callables);
        let lex = lex::register(&mut cx, &callables);
        let print = print::register(&mut cx, &callables);
        let error = error::register(&mut cx, &callables);
        let run_body = run_body::register(&mut cx);
        let here = here::register(&mut cx, &callables);
        // Last: the `binding` type's fields are `@dyad` places, so it waits for `dyad` and `@`.
        binding::register_type(&mut cx, scope_, array_, dyad_, numtypes[NumType::F64 as usize]);
        op_leaves.scope_ = scope::register_exec(&mut cx, scope_, &callables);
        let (ran_, ran_leaf) = ran::register(&mut cx, &callables);
        op_leaves.ran_ = ran_leaf;

        // Every constructor moves onto a callable leaf in its record; the table drops
        // before any parsing runs.
        let Cx { store, metas, lower, .. } = cx;
        // SAFETY: every key in `metas` has its record built, and each entry is a `ConstructFn`.
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
            mut_,
            immut_,
            shared_,
            import_,
            dyad_,
            binding_,
            colon_,
            tape,
            this,
            lex,
            print,
            error,
            run_body,
            here,
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
        binding::through(self.binding_, p)
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
    metas: HashMap<DyadPtr, ConstructFn>,
    lower: LowerTable,
}

impl Cx<'_> {
    /// Declare `spelling` at the root scope; returns the name's binding.
    pub(crate) fn declare(&mut self, spelling: &str, id: DyadPtr) -> DyadPtr {
        let root = self.root_scope;
        self.declare_in(root, spelling, id)
    }

    /// A native record type's field names live in the type's own scope.
    pub(crate) fn declare_in(&mut self, scope: DyadPtr, spelling: &str, id: DyadPtr) -> DyadPtr {
        let name = string::build_text(self.store, self.string_, spelling.as_bytes());
        let binding = Binding::alloc(self.store, self.binding_, Binding::new(id, scope, name));
        self.trie.insert(spelling, binding);
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
            let node = $build(p.store(), &types, id, lhs, rhs)?;
            tape.reduce_here(node);
            Ok(crate::parse::Constructed::Placed)
        }
        construct as crate::parse::ConstructFn
    }};
}
pub(crate) use infix_construct;

/// Exposed so the binary can drain the top level's `defer`s at program exit.
///
/// # Safety
/// `defer_node` must be a `defer` node from the store; `rt` its runtime.
pub unsafe fn run_deferred(
    rt: &mut crate::run::Runtime,
    defer_node: DyadPtr,
) -> Result<i64, crate::run::RunError> {
    rt.run(drop_model::deferred_inner_of(defer_node))
}

/// # Safety
/// `node.value` must point at an operand record of at least two `dyad@` fields.
pub(crate) unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
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

/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn numtype_of(types: &Core, node: DyadPtr) -> Operand {
    let node = types.through(node);
    let logos = (*node).ty;
    if logos == types.ran_ {
        return numtype_of(types, ran::expr_of(types, node));
    }
    if logos == types.rational {
        // A place of rational type holds a run-time rational, which no machine type takes silently.
        return if crate::dyad::is_place((*node).value) {
            Operand::NonNumeric
        } else {
            Operand::Literal
        };
    }
    // An arithmetic result has its left operand's type; the op slot holds the concrete op, not
    // a type.
    if logos == types.plus
        || logos == types.minus
        || logos == types.times
        || logos == types.div_
        || logos == types.rem_
    {
        let lhs = *((*node).value as *const DyadPtr);
        return numtype_of(types, lhs);
    }
    // A comparison or logical result is `bool`, physically an i32.
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
    // `and`/`or` over non-booleans is a group, no number at all.
    if logos == types.and_ || logos == types.or_ {
        return if crate::parse::is_bool_result(types, node) {
            Operand::Concrete(NumType::I32)
        } else {
            Operand::NonNumeric
        };
    }
    // A tape or `this` read yields a cell's address, an `@dyad` value: a write stores
    // what it yields, never its own address.
    if logos == types.tape.slot
        || logos == types.tape.cell_type
        || logos == types.tape.spelling
        || logos == types.tape.slot_name
        || logos == types.this.slot
    {
        return Operand::Pointer(types.dyad_);
    }
    // `here`, `caller.scope` and `.back` yield a node's address, as `x:scope` does.
    if logos == types.here.here || logos == types.here.caller_scope || logos == types.here.back {
        return Operand::Pointer(types.dyad_);
    }
    if logos == types.convert {
        return Operand::Concrete(numtype::of_type_node(numtype::stored_type(node)));
    }
    if types.numtypes.iter().any(|&t| !t.is_null() && t == logos) {
        return Operand::Concrete(numtype::of_type_node(logos));
    }
    // An else-less `if` yields unit; with both branches, the bare i32 default.
    if logos == types.if_ {
        if (*((*node).value as *const DyadPtr).add(2)).is_null() {
            return Operand::NonNumeric;
        }
        return Operand::Concrete(NumType::I32);
    }
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
    // `alloc`'s pointee sits at operand 0, `own`'s at 1; owning-ness rides the bound
    // place's type, not this result.
    if logos == types.alloc_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr));
    }
    if logos == types.own_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr).add(1));
    }
    if !logos.is_null() && numtype::is_pointer_type(logos) {
        return Operand::Pointer(numtype::pointee_of(logos));
    }
    // `&x` stores its pointee at operand 1; its node type is its own identity, not a pointer type.
    if logos == types.addr_ {
        return Operand::Pointer(*((*node).value as *const DyadPtr).add(1));
    }
    // Before the fn-typed fallback, which would misread these as i32-returning calls.
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
    // A literal tail commits as i32 here: molding commits a literal node, and this node is the
    // sequence.
    if logos == types.scope {
        return match crate::parse::last_sequence_expr(node) {
            Some(last) => match numtype_of(types, last) {
                Operand::Literal => Operand::Concrete(NumType::I32),
                other => other,
            },
            None => Operand::NonNumeric,
        };
    }
    // A call yields the callee's return type; a placeholder with no published signature
    // falls back to i32. A node of a type with a run reads as the function built for
    // its field types, or as nothing until one exists.
    let logos =
        if !logos.is_null() && meta::is_record_type(logos) && !meta::run_body_of(logos).is_null() {
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
            // A rational result has no machine form.
            if !out.is_null() && out == types.rational {
                return Operand::NonNumeric;
            }
        }
        return Operand::Concrete(call_return_numtype(logos));
    }
    Operand::NonNumeric
}

/// `I32` when the callee has no published signature.
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

/// Two different concrete types are a mismatch (no implicit coercion); a literal
/// commits to the other operand's type, two literals to i32.
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
        // A pointer step is built before this; any other pointer operand is refused.
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
        (Operand::Literal, Operand::Literal) => NumType::I32,
    };
    let type_node = types.numtypes[nt as usize];
    let lhs = commit_if_literal(store, types, lhs, &a, type_node, nt)?;
    let rhs = commit_if_literal(store, types, rhs, &b, type_node, nt)?;
    Ok(([lhs, rhs], nt))
}

unsafe fn commit_if_literal(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    op: &Operand,
    type_node: DyadPtr,
    nt: NumType,
) -> Result<DyadPtr, ParseError> {
    if let Operand::Literal = op {
        // A comptime name used here is its binding; the literal folds through it.
        let bits =
            rational::mold_to(types.through(node), nt).ok_or(ParseError::UncomputableLiteral)?;
        let value = store.alloc_bytes(&bits.to_ne_bytes()[..nt.bytes()]);
        Ok(store.alloc_raw(type_node, value))
    } else {
        Ok(node)
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

/// The place type and byte width a declaration's binding needs: a numeric at its
/// width, a pointer as a fresh `@pointee` place 8 bytes wide.
///
/// # Safety
/// `value` must be a reduced dyad from the store that `numtype_of` classifies as
/// concrete or pointer.
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

/// The initializing store of a declaration, the same node `=` builds.
pub(crate) fn build_init(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    assign::build(store, types, types.assign, place, value)
}

/// The no-coercion rule for `=`: a numeric target takes exactly its own type, a
/// pointer target a pointer to a matching pointee. Literals never reach here; the
/// callers commit them first.
///
/// # Safety
/// `target_ty` must be a numeric or pointer type node and `rhs` a reduced dyad,
/// both from the store.
pub(crate) unsafe fn check_store_type(
    types: &Core,
    target_ty: DyadPtr,
    rhs: DyadPtr,
) -> Result<(), ParseError> {
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

/// Same type, not same node: an owning `@T` and the plain `@T` name one pointee.
///
/// # Safety
/// `a`/`b` must be type nodes from the store.
unsafe fn pointee_types_match(a: DyadPtr, b: DyadPtr) -> bool {
    a == b
        || (numtype::is_pointer_type(a)
            && numtype::is_pointer_type(b)
            && pointee_types_match(numtype::pointee_of(a), numtype::pointee_of(b)))
}

/// Render a run result through `node`'s static type, so `5.5` and `true` print as such.
///
/// # Safety
/// `node` must be a valid dyad from the store, the expression whose value `bits` is.
pub unsafe fn display_value(types: &Core, node: DyadPtr, bits: i64) -> String {
    // A scope's value is its trailing expression; render that node.
    let node = types.through(trailing_expr(types, node));
    // A bool stored in a variable reads back as 0/1; only a direct bool expression renders as
    // truth.
    if crate::parse::is_bool_result(types, node) {
        return if bits != 0 { "true" } else { "false" }.to_string();
    }
    // A rational value's bits are the number's address.
    if rational::is_rational_value(types, node) {
        let held = bits as usize as DyadPtr;
        if !held.is_null() && (*held).ty == types.rational {
            return rational::spell(held);
        }
    }
    match read::read_kind(types, node) {
        read::Read::Identity => type_name(types, node),
        // A box's bits are the held node's address. Rendering a held dyad would mean
        // running it, and display has no runtime, so it shows as `dyad`.
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
                String::from_utf8_lossy(string::text(held)).into_owned()
            } else {
                "dyad".to_string()
            }
        }
        read::Read::Address => "dyad".to_string(),
        read::Read::Scalar(nt) => format_scalar(nt, bits),
        read::Read::Pointer(_) => format_scalar(NumType::U64, bits),
        _ => match numtype_of(types, node) {
            Operand::Concrete(nt) => format_scalar(nt, bits),
            _ => bits.to_string(),
        },
    }
}

/// The innermost trailing expression of a scope, whose value the run result is.
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

/// The `i32 32` juxtaposition: a literal committed exactly to a numeric type.
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

/// Commit a call's literal arguments to their parameters' declared numeric types, so
/// `f(3000000000)` is exact for an i64 parameter. Extra arguments are left for the
/// arity check.
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
        // A bare `name` parameter accepts any dyad.
        if pty.is_null() {
            continue;
        }
        // A record or code-carrying parameter has no whole read here; it is checked
        // where its type is built.
        match read::place_layout(types, pty) {
            // A literal committed into a pointer parameter would be dereferenced as a wild address.
            Some((read::Read::Pointer(pp), _)) => match numtype_of(types, *arg) {
                Operand::Pointer(pointee) if pointee_types_match(pp, pointee) => {}
                _ => return Err(ParseError::TypeMismatch),
            },
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
            // A number into a `type` parameter would travel as an address.
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

/// Commit the body's tail literals to the declared return type; a `-> type` body is
/// checked instead.
///
/// # Safety
/// `body`/`output` are valid dyads from the store.
pub(crate) unsafe fn commit_fn_body(
    store: &mut Store,
    types: &Core,
    body: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // A `-> type` result is read back as a node address, so every tail leaf must be a type.
    if output == types.type_ {
        check_type_tail(types, body)?;
        return Ok(body);
    }
    if !is_numtype_node(types, output) {
        return Ok(body);
    }
    commit_tail(store, types, body, output)
}

/// Walk the value-producing positions of `node` (itself, a `return`'s operand, both
/// `if` branches, a scope's trailing expression) and write back what `leaf` returns.
/// Statements are refused: they yield unit.
///
/// # Safety
/// `node` is a valid dyad from the store.
unsafe fn walk_tail(
    types: &Core,
    node: DyadPtr,
    leaf: &mut impl FnMut(DyadPtr) -> Result<DyadPtr, ParseError>,
) -> Result<DyadPtr, ParseError> {
    if (*node).ty == types.return_ {
        let ops = (*node).value as *mut DyadPtr;
        *ops = walk_tail(types, *ops, leaf)?;
        return Ok(node);
    }
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
    // Trailing prose is invisible to value flow, so the tail is the last non-comment expression.
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

/// A rational leaf molds to `output`, exactly or not at all; everything else passes through.
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
        // Refused here rather than as an invalid widen at the ABI.
        if let Operand::Pointer(_) = numtype_of(types, leaf) {
            return Err(ParseError::TypeMismatch);
        }
        // A run-time rational has no machine form until it is converted explicitly.
        if rational::is_rational_value(types, leaf) {
            return Err(ParseError::TypeMismatch);
        }
        Ok(leaf)
    })
}

/// A `-> type` call's result bits are read as a node address, so a non-type tail
/// would be a wild address.
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

/// The `T(value)` conversion, the only cross-type path: a literal folds now with `as`
/// semantics, a runtime operand of another type becomes a `convert` node, the same
/// type passes through.
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
        Operand::Concrete(from) => {
            if from == to {
                Ok(operand)
            } else {
                let from_node = types.numtypes[from as usize];
                Ok(convert::build_convert(store, types, operand, from_node, target))
            }
        }
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
