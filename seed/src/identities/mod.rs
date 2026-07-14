// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The seed's hand-built core identities, one file each.
//!
//! Everything in Logos is an identity, but only the seed's *native* identities
//! are authored in Rust; identities created while a program runs are graph data,
//! never source files. This folder is that bounded native kernel: the node cell
//! ([`dyad`]) and name-resolution pairing ([`id_context`]) the substrate is
//! built from, plus each primitive (`type`, `fn`, `i32`, `rational`, `=`, `+`).
//!
//! Each primitive file defines exactly one identity: its node, its spelling, and
//! its behaviour across the phases (parse `Construct`, run `bcode`, compile
//! lowering). [`Core::build`] wires them into the graph and the per-phase tables.
//! The split is structure vs. behaviour: every identity's *structure* — its
//! parse precedence and associativity, and the layout its values are read
//! through (a scalar width, operand arity and role names) — rides the graph as
//! its shared-member record ([`meta`]; DESIGN ›A type's metadata is shared by
//! its values‹), while the native *behaviour* stays in the per-phase tables
//! (parser/run/compile) rather than on the nodes, so a new run or compile
//! *version* is a new table, not a graph rewrite.
//!
//! Deferred surface the sketch declares that the seed does not yet register —
//! tracked here so each gap is deliberate, not drift: the operators `^` and
//! `xor`; ranges as first-class values, and `for`'s multi-variable, `in`-less,
//! and `gpu` forms (the old prototype has them); pointer arithmetic, heap
//! allocation, and checked `&mut` references (see `pointer`); struct-typed
//! (nested) fields and struct parameters/returns; the `string` *name* and operations over
//! strings (the `«…»` literal exists as an inert value, above all as the comment
//! substance); `mut` at every level (DESIGN ›Mutability and construction‹); the
//! `hashtable` and `array` types; and the declaration forms `key : type = value`
//! and bare `key :` outside field lists. Each arrives with the machinery it
//! needs (layout, places, the borrow rule), not before.

use std::collections::HashMap;

use crate::compile::LowerTable;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError, FN_OUTPUT};
use crate::run::Bcode;
use crate::regex_trie::RegexTrie;
use crate::store::Store;

pub use numtype::NumType;

pub mod dyad;
pub mod id_context;

#[path = "type.rs"]
mod type_mod;
#[path = "fn.rs"]
mod fn_mod;
#[path = "return.rs"]
mod return_mod;
#[path = "struct.rs"]
mod struct_mod;
#[path = "bool.rs"]
mod bool_mod;
#[path = "if.rs"]
mod if_mod;
#[path = "while.rs"]
mod while_mod;
#[path = "for.rs"]
mod for_mod;
mod and;
mod assign;
mod comment;
mod convert;
mod declare;
mod divide;
pub(crate) mod instance;
pub(crate) mod pointer;
mod eq;
mod ge;
mod gt;
mod le;
mod lt;
pub(crate) mod meta;
mod minus;
mod modulo;
mod ne;
mod not;
pub(crate) mod numtype;
mod or;
mod paren;
mod plus;
pub(crate) mod rational;
mod scope;
pub(crate) mod string;
mod times;

/// The core identities and the per-phase tables that drive them.
pub struct Core {
    /// The `Type : Type` self-loop, the one node whose type is itself.
    pub type_: DyadPtr,
    /// `scope`, the type of a scope node (the graph's spine). Each scope the parser
    /// opens is typed with this.
    pub scope_: DyadPtr,
    /// The scope every core identity is declared in; itself a `scope`-typed node.
    pub root_scope: DyadPtr,
    /// `fn`, the type whose values are functions.
    pub fn_type: DyadPtr,
    /// `i32`, the type of an integer variable/value (an alias for `numtypes[I32]`).
    pub i32_: DyadPtr,
    /// The numeric primitive type nodes, indexed by `NumType`. Unregistered types
    /// (e.g. `f32`/`f64` before their phase) are null.
    pub numtypes: [DyadPtr; 10],
    /// `bool`, the type of a boolean value (a comparison result; an `if` condition).
    pub bool_: DyadPtr,
    /// `void`, the zero-sized unit type: a `-> void` function yields unit (0 bits).
    pub void: DyadPtr,
    /// `=` (assignment); a function.
    pub assign: DyadPtr,
    /// `convert`: the shared scalar numeric conversion, built from a `type(value)`
    /// constructor and carrying its source/target types per node.
    pub convert: DyadPtr,
    /// `+` (addition); resolves and stores its operand type per node.
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
    /// `rational_number` (numeric literal carrier); a data type.
    pub rational: DyadPtr,
    /// `string` (the `«…»` text literal); inert in the seed, the comment substance.
    pub string_: DyadPtr,
    /// `comment` (the prose node a statement-level `#` builds); invisible to
    /// value flow.
    pub comment_: DyadPtr,
    /// `struct`, the type whose constructor derives a layout from a field list
    /// (and whose field list is a function's parameter list).
    pub struct_: DyadPtr,
    /// `construct`, the struct-construction statement a struct-typed call builds.
    pub construct_: DyadPtr,
    /// `deref`, the dereference node postfix `@` builds.
    pub deref_: DyadPtr,
    /// `storeptr`, the store-through node `=` builds over a deref lhs.
    pub storeptr_: DyadPtr,
    /// The parser's table: parse-time behaviour keyed by identity.
    pub metas: HashMap<DyadPtr, Construct>,
    /// One run version: each function identity's `bcode`.
    pub bcode: Bcode,
    /// One compile version: each operation's Cranelift lowering rule.
    pub lower: LowerTable,
}

impl Core {
    /// Hand-build the core graph into `store`, registering spellings in `trie`.
    pub fn build(store: &mut Store, trie: &mut RegexTrie) -> Core {
        // Foundational types first: others reference them.
        let type_ = type_mod::register_root(store);
        let scope_ = scope::register(store, type_);
        let root_scope = store.alloc_raw(scope_, std::ptr::null_mut());
        let fn_type = fn_mod::register(store, type_);

        // Then the behaviour-bearing identities, via a shared build context.
        let mut cx = Cx {
            store,
            trie,
            type_,
            fn_type,
            root_scope,
            string_: std::ptr::null_mut(),
            metas: HashMap::new(),
            bcode: HashMap::new(),
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
        // `void`: the zero-sized unit type (a `-> void` return). Self-describing via a
        // tag past the numeric range, so run/compile recognize it without a handle.
        let void = numtype::register_void(&mut cx);
        let bool_ = bool_mod::register(&mut cx);
        let rational = rational::register(&mut cx);
        // The text substance: `«…»` string literals and the comment nodes a
        // statement-level `#` builds over them. Registered before the operators,
        // whose records name their operands with string nodes.
        let string_ = string::register(&mut cx);
        cx.string_ = string_;
        let comment_ = comment::register(&mut cx);
        // The foundations allocated before the build context get their records
        // now: `type`'s values are types carrying records like its own (the
        // fixed point), a `scope`'s value is the null-terminated expression list.
        let record = meta::record(cx.store, meta::TYPEREC_TAG);
        // SAFETY: `type_`/`scope_` were allocated above with null value slots
        // nothing has read yet.
        unsafe {
            (*type_).value = record;
        }
        let record = meta::operand_record(&mut cx, meta::LIST_TAG, 0.0, Assoc::Left, &[]);
        unsafe {
            (*scope_).value = record;
        }
        let assign = assign::register(&mut cx);
        // The shared scalar numeric conversion (`i32(a)`, `f64(x)`, …). No spelling; the
        // parser builds conversion nodes from the `type(value)` constructor surface.
        let convert = convert::register(&mut cx);
        // The numeric operators. Each resolves its operand type at parse time and
        // stores it in the node's value slot; run/compile switch on it (see
        // `numtype`), so one identity per operator serves every numeric type.
        let plus = plus::register(&mut cx);
        let minus = minus::register(&mut cx);
        let times = times::register(&mut cx);
        let div_ = divide::register(&mut cx);
        let rem_ = modulo::register(&mut cx);
        let lt = lt::register(&mut cx);
        let gt = gt::register(&mut cx);
        let eq = eq::register(&mut cx);
        let le = le::register(&mut cx);
        let ge = ge::register(&mut cx);
        let ne = ne::register(&mut cx);
        // The logical operators, over `bool` (their operands are comparisons/bools).
        let and_ = and::register(&mut cx);
        let or_ = or::register(&mut cx);
        let not_ = not::register(&mut cx);
        let if_ = if_mod::register(&mut cx);
        let while_ = while_mod::register(&mut cx);
        let for_ = for_mod::register(&mut cx);
        fn_mod::register_syntax(&mut cx);
        paren::register(&mut cx);
        let return_ = return_mod::register(&mut cx);
        // `:=` is a parse-time-only token; the driver dispatches on its Construct.
        declare::register(&mut cx);
        let struct_ = struct_mod::register(&mut cx);
        // Struct instances: the construction statement and the `.` field access.
        let construct_ = instance::register(&mut cx);
        // Pointers: the `@`/`&` tokens and the deref/storeptr identities.
        let (deref_, storeptr_) = pointer::register(&mut cx);
        // A multi-expression block is a `scope`-typed sequence node; its run and
        // lowering are registered once the tables exist.
        scope::register_exec(&mut cx, scope_);

        let Cx { metas, bcode, lower, .. } = cx;
        Core {
            type_,
            scope_,
            root_scope,
            fn_type,
            i32_,
            numtypes,
            bool_,
            void,
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
            rational,
            string_,
            comment_,
            struct_,
            construct_,
            deref_,
            storeptr_,
            metas,
            bcode,
            lower,
        }
    }

    /// The core type handles the parser needs to type the nodes it opens and to
    /// resolve abstract operators.
    pub fn types(&self) -> CoreTypes {
        CoreTypes {
            scope: self.scope_,
            struct_: self.struct_,
            fn_type: self.fn_type,
            i32_: self.i32_,
            numtypes: self.numtypes,
            bool_: self.bool_,
            rational: self.rational,
            return_: self.return_,
            if_: self.if_,
            while_: self.while_,
            for_: self.for_,
            type_: self.type_,
            deref_: self.deref_,
            storeptr_: self.storeptr_,
            construct_: self.construct_,
            string_: self.string_,
            comment_: self.comment_,
            convert: self.convert,
            plus: self.plus,
            minus: self.minus,
            times: self.times,
            div_: self.div_,
            rem_: self.rem_,
            lt: self.lt,
            gt: self.gt,
            eq: self.eq,
            le: self.le,
            ge: self.ge,
            ne: self.ne,
            and_: self.and_,
            or_: self.or_,
            not_: self.not_,
        }
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
    /// The `string` type, once registered (null before): an operand record's role
    /// names are string nodes, so the identities registered after it can name
    /// their operands as graph data.
    string_: DyadPtr,
    metas: HashMap<DyadPtr, Construct>,
    bcode: Bcode,
    lower: LowerTable,
}

/// Build a plain binary application `{ty: op, value: [lhs, rhs]}`, used by `=`
/// (which is not type-resolved). `+` uses its own resolving builder instead (see
/// [`plus`]); the shared `CoreTypes` parameter lets a builder resolve operand types
/// but is unused here.
pub(crate) fn build_binary(
    store: &mut Store,
    _types: &CoreTypes,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let operands = store.alloc_operands(&[lhs, rhs]);
    Ok(store.alloc_raw(op, operands))
}

/// The two `dyad@` operands of a binary application node.
///
/// # Safety
/// `node.value` must point at an operand struct of at least two `dyad@` fields,
/// as produced by [`build_binary`].
pub(crate) unsafe fn operands(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = (*node).value as *const DyadPtr;
    (*p, *p.add(1))
}

/// A binary numeric operator operand's character, for type resolution.
pub(crate) enum Operand {
    /// A value with a committed numeric type.
    Concrete(NumType),
    /// An uncommitted number literal (a `rational`), which molds to context.
    Literal,
    /// A pointer value, carrying its pointee type node. Pointer types compare by
    /// pointee, never by node identity — they are created fresh per use.
    Pointer(DyadPtr),
    /// Not a number an operator can compute over (e.g. a `struct`).
    NonNumeric,
}

/// Classify `node` as an operand of a numeric operator: its committed type, an
/// uncommitted literal, or non-numeric.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn numtype_of(types: &CoreTypes, node: DyadPtr) -> Operand {
    let ty = (*node).ty;
    if ty == types.rational {
        return Operand::Literal;
    }
    // An arithmetic operator's result carries the type it stored at operand[2].
    if ty == types.plus || ty == types.minus || ty == types.times || ty == types.div_ || ty == types.rem_ {
        return Operand::Concrete(numtype::of_type_node(numtype::stored_type(node)));
    }
    // A conversion's result is its target type (stored at operand[2]).
    if ty == types.convert {
        return Operand::Concrete(numtype::of_type_node(numtype::stored_type(node)));
    }
    // A numeric variable/value: its type is one of the numeric type nodes.
    if types.numtypes.iter().any(|&t| !t.is_null() && t == ty) {
        return Operand::Concrete(numtype::of_type_node(ty));
    }
    // An else-less `if` yields unit, not a value (it has no false branch to produce
    // one); with both branches it resolves through the fn-typed default below (the
    // `if` identity is itself fn-typed).
    if ty == types.if_ && (*((*node).value as *const DyadPtr).add(2)).is_null() {
        return Operand::NonNumeric;
    }
    // A `while`/`for` loop and a struct construction are statements yielding
    // unit, never values.
    if ty == types.while_ || ty == types.for_ || ty == types.construct_ {
        return Operand::NonNumeric;
    }
    // A pointer-typed value (an `&x` literal, a pointer variable, or a pointer
    // field place): carries its pointee. Never arithmetic; passed and stored whole.
    if !ty.is_null() && numtype::is_pointer_type(ty) {
        return Operand::Pointer(numtype::pointee_of(ty));
    }
    // A dereference yields its pointee's value; a store-through yields the stored
    // value, like `=`. Both must precede the generic fn-typed fallback below,
    // which would misread them as i32-returning calls.
    if ty == types.deref_ || ty == types.storeptr_ {
        let p = (*node).value as *const DyadPtr;
        let pointee = if ty == types.deref_ { *p.add(1) } else { *p.add(2) };
        if numtype::is_pointer_type(pointee) {
            return Operand::Pointer(numtype::pointee_of(pointee));
        }
        if is_numtype_node(types, pointee) {
            return Operand::Concrete(numtype::of_type_node(pointee));
        }
        return Operand::NonNumeric; // a struct pointee reads only through `.field`
    }
    // A sequence's value is its trailing expression's. A literal tail takes the
    // bare-literal i32 default here rather than classifying as `Literal`: the
    // molding machinery commits a literal *node*, and this node is the sequence.
    if ty == types.scope {
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
    if !ty.is_null() && (*ty).ty == types.fn_type {
        // A core operator identity (`=`, a comparison, `return`, `not`, `and`,
        // `or`) carries its operand record, not an fn record; its applications
        // keep the bare i32 default they always had (a comparison's bool is
        // physically an i32).
        if meta::is_operand_record(ty) {
            return Operand::Concrete(NumType::I32);
        }
        let fields = (*ty).value as *const DyadPtr;
        if !fields.is_null() {
            let out = *fields.add(FN_OUTPUT);
            if !out.is_null() && numtype::is_void_type(out) {
                return Operand::NonNumeric;
            }
            if !out.is_null() && numtype::is_pointer_type(out) {
                return Operand::Pointer(numtype::pointee_of(out));
            }
        }
        return Operand::Concrete(call_return_numtype(ty));
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
        numtype::numtype_of_type(out)
    }
}

/// Resolve a binary numeric operator's operand type and produce its
/// `[lhs, rhs, type]` value, committing any uncommitted literal operand to the
/// resolved type. Two different concrete types are a [`ParseError::TypeMismatch`]
/// (cross-type needs an explicit cast); a non-numeric operand is
/// [`ParseError::UnsupportedOperands`]; a literal that has no exact value in the
/// resolved type is [`ParseError::UncomputableLiteral`].
///
/// # Safety
/// `lhs`/`rhs` are valid dyads from the store.
pub(crate) unsafe fn resolve_binary(
    store: &mut Store,
    types: &CoreTypes,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<[DyadPtr; 3], ParseError> {
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
    let lhs = commit_if_literal(store, lhs, &a, type_node, nt)?;
    let rhs = commit_if_literal(store, rhs, &b, type_node, nt)?;
    Ok([lhs, rhs, type_node])
}

/// Commit an uncommitted literal operand to `nt` (a typed literal node holding the
/// molded bytes); non-literal operands pass through unchanged.
unsafe fn commit_if_literal(
    store: &mut Store,
    node: DyadPtr,
    op: &Operand,
    type_node: DyadPtr,
    nt: NumType,
) -> Result<DyadPtr, ParseError> {
    if let Operand::Literal = op {
        let bits = rational::mold_to(node, nt).ok_or(ParseError::UncomputableLiteral)?;
        let value = store.alloc_bytes(&bits.to_ne_bytes()[..nt.bytes()]);
        Ok(store.alloc_raw(type_node, value))
    } else {
        Ok(node)
    }
}

/// Whether `node` is one of the registered numeric type nodes (`i32`, `f64`, …). The
/// parser uses this to recognize a `type(value)` conversion at a call site.
pub(crate) fn is_numtype_node(types: &CoreTypes, node: DyadPtr) -> bool {
    types.numtypes.iter().any(|&t| !t.is_null() && t == node)
}

/// Resolve a `for` range's operand type across its parts (start, end, optional
/// step), like [`resolve_binary`] over more operands: concrete types must all
/// match ([`ParseError::TypeMismatch`]), literals commit in place to the
/// resolved type, all-literals default to i32, and a non-numeric part is
/// rejected. Returns the resolved numeric type node.
///
/// # Safety
/// `parts` must be reduced dyads from the store.
pub(crate) unsafe fn resolve_loop_parts(
    store: &mut Store,
    types: &CoreTypes,
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
    let ty = types.numtypes[nt as usize];
    for p in parts.iter_mut() {
        if let Operand::Literal = numtype_of(types, *p) {
            *p = commit_if_literal(store, *p, &Operand::Literal, ty, nt)?;
        }
    }
    Ok(ty)
}

/// Commit a rational literal node exactly to the numeric type `ty_node` — the
/// `type literal` juxtaposition (`i32 32`, DESIGN ›an anonymous typed value is
/// written by juxtaposition‹). The result is a typed value with real storage.
///
/// # Safety
/// `lit` must be a rational literal from the store; `ty_node` a numeric type node.
pub(crate) unsafe fn commit_literal_to(
    store: &mut Store,
    lit: DyadPtr,
    ty_node: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let nt = numtype::of_type_node(ty_node);
    commit_if_literal(store, lit, &Operand::Literal, ty_node, nt)
}

/// Commit a call's uncommitted literal arguments to their parameters' declared
/// numeric types — the typed slot (DESIGN ›committing to a concrete type only when
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
    types: &CoreTypes,
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
    let params = (*(*fields.add(crate::parse::FN_INPUT))).value as *const DyadPtr;
    if params.is_null() {
        return Ok(());
    }
    for (i, arg) in args.iter_mut().enumerate() {
        let param = *params.add(i + 1); // [scope, p0 …, null]
        if param.is_null() {
            break;
        }
        let pty = (*param).ty;
        if !pty.is_null() && numtype::is_pointer_type(pty) {
            // A pointer parameter takes only a pointer to the same pointee — a
            // committed literal here would be dereferenced as a wild address.
            match numtype_of(types, *arg) {
                Operand::Pointer(pointee) if pointee == numtype::pointee_of(pty) => {}
                _ => return Err(ParseError::TypeMismatch),
            }
            continue;
        }
        if (**arg).ty == types.rational && is_numtype_node(types, pty) {
            let nt = numtype::of_type_node(pty);
            *arg = commit_if_literal(store, *arg, &Operand::Literal, pty, nt)?;
        }
    }
    Ok(())
}

/// Commit a comptime-rational function body to its declared return type — the typed-slot
/// context (DESIGN ›a rational commits when it lands in a typed slot‹). A `void` return,
/// or any non-concrete output, passes the body through; otherwise the body's tail value
/// positions are committed (see [`commit_tail`]).
///
/// # Safety
/// `body`/`output` are valid dyads from the store.
pub(crate) unsafe fn commit_fn_body(
    store: &mut Store,
    types: &CoreTypes,
    body: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if !is_numtype_node(types, output) {
        return Ok(body);
    }
    commit_tail(store, types, body, output)
}

/// Commit a comptime rational in tail (value-producing) position to `output`, a numeric
/// type node. The tail positions are the leaves a function's value can come from: the
/// node itself, the operand of a `return`, and *both* branches of an `if` — recursively,
/// so `return (if …)`, nested `if`s, and the like all reach their leaves. A rational
/// leaf molds to `output` (exact, else [`ParseError::UncomputableLiteral`]); everything
/// else passes through. The value-producing constructs are enumerated here because the
/// seed has no graph-driven value-slot machinery yet (that arrives with self-hosting);
/// the branch node is mutated in place, which is safe since it was just parsed and is not
/// yet aliased.
///
/// # Safety
/// `node`/`output` are valid dyads from the store; `output` is a numeric type node.
unsafe fn commit_tail(
    store: &mut Store,
    types: &CoreTypes,
    node: DyadPtr,
    output: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if (*node).ty == types.rational {
        let nt = numtype::of_type_node(output);
        let bits = rational::mold_to(node, nt).ok_or(ParseError::UncomputableLiteral)?;
        let value = store.alloc_bytes(&bits.to_ne_bytes()[..nt.bytes()]);
        return Ok(store.alloc_raw(output, value));
    }
    // `return X`: X is the tail (the node's `value` *is* that operand).
    if (*node).ty == types.return_ {
        let operand: DyadPtr = (*node).value.cast();
        let committed = commit_tail(store, types, operand, output)?;
        (*node).value = committed.cast();
        return Ok(node);
    }
    // `if (c) (then) else (else)`: both branches are tails (value `[cond, then, else]`).
    // An else-less `if` yields unit, so it cannot be a numeric function's tail.
    if (*node).ty == types.if_ {
        let ops = (*node).value as *mut DyadPtr;
        if (*ops.add(2)).is_null() {
            return Err(ParseError::MissingElse);
        }
        let then_c = commit_tail(store, types, *ops.add(1), output)?;
        let else_c = commit_tail(store, types, *ops.add(2), output)?;
        *ops.add(1) = then_c;
        *ops.add(2) = else_c;
        return Ok(node);
    }
    // A `while`/`for` loop or a construction yields unit, so none of them can be
    // a numeric function's tail.
    if (*node).ty == types.while_
        || (*node).ty == types.for_
        || (*node).ty == types.construct_
    {
        return Err(ParseError::StatementAsValue);
    }
    // A sequence: its trailing non-comment expression is the tail (trailing prose
    // is invisible to value flow).
    if (*node).ty == types.scope {
        let ops = (*node).value as *mut DyadPtr;
        if !ops.is_null() {
            let mut i = 0;
            while !(*ops.add(i)).is_null() {
                i += 1;
            }
            while i > 0 {
                let cand = *ops.add(i - 1);
                if !numtype::is_comment_type((*cand).ty) {
                    let committed = commit_tail(store, types, cand, output)?;
                    *ops.add(i - 1) = committed;
                    break;
                }
                i -= 1;
            }
        }
        return Ok(node);
    }
    // A pointer cannot be a numeric function's value (commit_tail runs only for
    // numeric outputs); rejecting here beats an invalid widen at the ABI.
    if let Operand::Pointer(_) = numtype_of(types, node) {
        return Err(ParseError::TypeMismatch);
    }
    Ok(node)
}

/// Build a scalar numeric conversion `target(operand)` — the `type(value)` constructor
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
    types: &CoreTypes,
    target: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let [operand] = args else {
        return Err(ParseError::BadCast);
    };
    let operand = *operand;
    let to = numtype::of_type_node(target);
    match numtype_of(types, operand) {
        // A runtime value: a same-type cast is a no-op, a different type converts.
        Operand::Concrete(from) => {
            if from == to {
                Ok(operand)
            } else {
                let from_node = types.numtypes[from as usize];
                Ok(convert::build_convert(store, types.convert, operand, from_node, target))
            }
        }
        // A literal: fold it into a `target`-typed value now, with `as` semantics.
        Operand::Literal => {
            let bits = rational::cast_to(operand, to).ok_or(ParseError::UncomputableLiteral)?;
            let value = store.alloc_bytes(&bits.to_ne_bytes()[..to.bytes()]);
            Ok(store.alloc_raw(target, value))
        }
        // Pointer-to-integer casts are deferred with the rest of pointer math.
        Operand::Pointer(_) | Operand::NonNumeric => Err(ParseError::BadCast),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile_fn, compile_nullary_i32};
    use crate::parse::{Parser, ScopeStack, FN_BCODE, FN_BODY, FN_INPUT, FN_OUTPUT};
    use crate::run::Runtime;

    #[test]
    fn parses_a_equals_a_plus_one() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // Declare the variable `a` in the root scope.
        let a = store.alloc_raw(core.i32_, std::ptr::null_mut());
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // Expect the tree =(a, +(a, 1)).
        unsafe {
            assert_eq!((*root).ty, core.assign);
            let top = (*root).value as *const DyadPtr;
            assert_eq!(*top, a); // =.lhs is the variable a
            let sum = *top.add(1); // =.rhs is the + application
            assert_eq!((*sum).ty, core.plus);
            let sops = (*sum).value as *const DyadPtr;
            assert_eq!(*sops, a); // +.lhs is a
            // +.rhs is the literal `1`, committed to i32 (the type resolved from `a`).
            let one = *sops.add(1);
            assert_eq!((*one).ty, core.i32_);
            assert_eq!(std::ptr::read_unaligned((*one).value as *const i32), 1);
            // `+` stayed reflectable (ty is still `+`) and stored the resolved operand
            // type as its third operand.
            assert_eq!(*sops.add(2), core.i32_);
        }
    }

    #[test]
    fn runs_a_equals_a_plus_one() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // `a` is an i32 variable initialised to 0.
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // run `a = a + 1`: yields 1 and leaves a holding 1.
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `root` is the valid dyad tree just parsed into `store`.
        let result = unsafe { rt.run(root) }.unwrap();
        assert_eq!(result, 1);
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 1);
        }
    }

    #[test]
    fn runs_a_compound_function_by_walking_its_body() {
        // A function with no bcode is interpreted by walking its `body` field. The
        // body `return a + 1` reads an enclosing variable, so the walk resolves `a`
        // and loads it (a non-trivial body, and a valid one: it returns its i32).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let main = {
            let mut p = Parser::new(
                "fn () -> i32 ( return a + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        // A nullary application of `main`: its type is `main`.
        let call = store.alloc_raw(main, std::ptr::null_mut());

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`main`/body are valid nodes in `store`.
        let result = unsafe { rt.run(call) }.unwrap();
        assert_eq!(result, 42); // a + 1 = 41 + 1
        unsafe {
            assert_eq!(std::ptr::read_unaligned(a_val as *const i32), 41); // a unchanged
        }
    }

    #[test]
    fn runs_a_returning_scope() {
        // `( return 40 + 2 )`: the brackets delimit a scope; `return` yields its
        // value. Runs to 42.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("( return 40 + 2 )", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the valid dyad tree just parsed.
        let result = unsafe { rt.run(node) }.unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn nested_scopes_and_bare_return() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Bare `return` (no brackets) yields to the top-level expression.
        let bare = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("return 7", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        assert_eq!(unsafe { rt.run(bare) }.unwrap(), 7);

        // Nested brackets group correctly.
        let nested = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("( ( return 5 ) )", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(unsafe { rt.run(nested) }.unwrap(), 5);
    }

    #[test]
    fn unclosed_bracket_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new("( return 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnclosedBracket));
    }

    #[test]
    fn parses_and_runs_a_fn() {
        // The real fn surface `fn ( params ) -> ret ( body )`. A nullary function
        // returning i32; applying and running it walks the body -> 42.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( return 40 + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        // Node shape: `{ty: fn, value -> [input, output, body, bcode]}` with an empty
        // input struct, an i32 return type, and a body (the `return`).
        unsafe {
            assert_eq!((*func).ty, core.fn_type);
            let v = (*func).value as *const DyadPtr;
            let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
            assert_eq!((*input).ty, core.struct_); // input is a struct
            let iops = (*input).value as *const DyadPtr;
            assert!((*iops.add(1)).is_null()); // no fields (scope then terminator)
            assert_eq!(output, core.i32_); // return type i32
            assert!(!body.is_null());
        }

        // Apply it and run: run finds no bcode for `func` and walks its body.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes in `store`.
        let result = unsafe { rt.run(call) }.unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn parses_a_fn_with_a_param_visible_in_the_body() {
        // A parameter is declared in the input struct's scope and resolves inside
        // the body: `fn (x : i32) -> i32 ( return x )` parses to a body `return(x)`
        // whose operand is the `x` parameter field. (Running it needs the calling
        // convention — param frame slots — which is later; here we check parsing.)
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn (x : i32) -> i32 ( return x )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        unsafe {
            assert_eq!((*func).ty, core.fn_type);
            let v = (*func).value as *const DyadPtr;
            let (input, output, body) = (*v.add(FN_INPUT), *v.add(FN_OUTPUT), *v.add(FN_BODY));
            assert_eq!(output, core.i32_);
            // The single parameter `x`, an i32 field in the input struct.
            let iops = (*input).value as *const DyadPtr;
            let x_field = *iops.add(1); // [scope, x, null]
            assert_eq!((*x_field).ty, core.i32_);
            // The body `return x` resolved `x` to that parameter field.
            let return_operand = (*body).value as DyadPtr;
            assert_eq!(return_operand, x_field);
        }
    }

    #[test]
    fn compiles_and_runs_a_fn_with_arguments() {
        // Step B: compile a two-parameter function and call it compiled, diffed
        // against the interpreter. Parameters lower to the function's arguments, so
        // the same `run(call)` that interpreted `add(40, 2)` now calls native code.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p =
                Parser::new("add(40, 2)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Oracle: interpret the call.
        // SAFETY: `call`/`add`/args are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();

        // Compile `add` (installs parameterized bcode); keep the artifact alive.
        // SAFETY: `add` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, core.types(),add) }.unwrap();
        // JIT: the same `run(call)` now evaluates the arguments and calls native code.
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 42);
        assert_eq!(jit, interp); // compiled parameterized call matches the oracle
    }

    #[test]
    fn fn_without_arrow_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new(
            "fn () ( return 1 )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            scopes,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedArrow));
    }

    #[test]
    fn calls_a_function_with_arguments() {
        // The calling convention (interpreted): define a two-parameter function,
        // call it with arguments, and read the parameters in the body. `add(40, 2)`
        // binds x=40, y=2 in a frame and the body `return x + y` reads them.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Define `add` (its params live in its own scope).
        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        // Declare `add`, then parse and run the call `add(40, 2)`.
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p =
                Parser::new("add(40, 2)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        // The call node applies `add` to its two arguments.
        unsafe {
            assert_eq!((*call).ty, add);
        }

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`add`/args are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
    }

    #[test]
    fn calling_with_the_wrong_arity_errors() {
        // Too few arguments for the parameters is a run error, not a bad read.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( return x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p = Parser::new("add(40)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`add` are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }, Err(crate::run::RunError::ArityMismatch));
    }

    #[test]
    fn fn_body_return_is_optional() {
        // `return` is optional: a body is valued by what it evaluates to, so a bare
        // `( 40 + 2 )` yields 42 just like `( return 40 + 2 )` does.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( 40 + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 42);
    }

    #[test]
    fn parses_an_empty_struct() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("struct ()", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // `{ty: struct, value -> [scope, null]}`: a scope, zero fields.
        unsafe {
            assert_eq!((*node).ty, core.struct_);
            let ops = (*node).value as *const DyadPtr;
            assert!(!(*ops).is_null()); // scope at index 0
            assert!((*ops.add(1)).is_null()); // terminator: no fields
        }
    }

    #[test]
    fn parses_a_struct_with_typed_fields() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p =
                Parser::new("struct (x : i32, y : i32)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // Two `:` declaration fields, each typed i32 with an undefined value.
        let (scope, fx, fy) = unsafe {
            assert_eq!((*node).ty, core.struct_);
            let ops = (*node).value as *const DyadPtr;
            assert!((*ops.add(3)).is_null()); // terminator after two fields
            (*ops, *ops.add(1), *ops.add(2))
        };
        unsafe {
            assert_eq!((*fx).ty, core.i32_);
            assert!((*fx).value.is_null());
            assert_eq!((*fy).ty, core.i32_);
            assert!((*fy).value.is_null());
        }

        // The field names are declared in the struct's own scope (index 0).
        let mut inner = ScopeStack::new();
        inner.push(scope);
        assert_eq!(inner.resolve(&trie, "x").unwrap().identity, fx);
        assert_eq!(inner.resolve(&trie, "y").unwrap().identity, fy);
    }

    #[test]
    fn parses_a_bare_name_field() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("struct (t)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // A bare name: one field with an undefined type slot.
        let (scope, ft) = unsafe {
            let ops = (*node).value as *const DyadPtr;
            assert!((*ops.add(2)).is_null()); // terminator after one field
            (*ops, *ops.add(1))
        };
        unsafe {
            assert!((*ft).ty.is_null()); // bare name: type undefined
            assert!((*ft).value.is_null());
        }

        let mut inner = ScopeStack::new();
        inner.push(scope);
        assert_eq!(inner.resolve(&trie, "t").unwrap().identity, ft);
    }

    #[test]
    fn struct_without_parens_is_an_error() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p = Parser::new("struct 40", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedOpen));
    }

    #[test]
    fn scopes_are_typed_scope() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // `scope` is a type (its own type is `type`), and the root scope is one.
        unsafe {
            assert_eq!((*core.scope_).ty, core.type_);
            assert_eq!((*core.root_scope).ty, core.scope_);
        }

        // A struct opens its own `scope`-typed node (value[0]).
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let node = {
            let mut p =
                Parser::new("struct (x : i32)", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        unsafe {
            let scope = *((*node).value as *const DyadPtr);
            assert_eq!((*scope).ty, core.scope_);
        }
    }

    #[test]
    fn jit_matches_the_interpreter() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let root = {
            let mut p = Parser::new("a = a + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };

        // Oracle: the interpreter, from a = 0.
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `root` is the valid tree just parsed.
        let interp = unsafe { rt.run(root) }.unwrap();
        let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        // Reset a to 0, then JIT-compile and call, and diff against the oracle.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 0) };
        // SAFETY: `root`/`a` live in `store`, which outlives the call.
        let compiled = unsafe { compile_nullary_i32(&core.lower, core.types(),root) }.unwrap();
        let jit = unsafe { compiled.call() };
        let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };

        assert_eq!(interp, 1);
        assert_eq!(jit, interp); // same result
        assert_eq!(jit_a, interp_a); // same side effect on a
        assert_eq!(jit_a, 1);
    }

    #[test]
    fn assign_to_a_wide_variable_stores_at_full_width_both_tiers() {
        // A STORED variable wider than i32 must be written at its full width, not
        // truncated to i32. `a : i64` starts at 0; `a = a + 5_000_000_000` must leave
        // the full 5e9 (0x1_2A05F200) — a 4-byte store would drop the high word and
        // leave 705_032_704. This is the case params (frame-bound i64) never exercised:
        // real backing storage assigned through.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&0i64.to_ne_bytes());
        let a = store.alloc_raw(core.numtypes[NumType::I64 as usize], a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        // A nullary `-> i64` fn so the compiled return type is the declared i64
        // (`compile_fn` reads `FN_OUTPUT`); its body assigns into the enclosing `a`.
        let func = {
            let mut p = Parser::new(
                "fn () -> i64 ( a = a + 5000000000 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Interpreted oracle: 0 + 5e9, and `a` now holds it.
        // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
        let interp = unsafe { rt.run(call) }.unwrap();
        let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i64) };

        // Reset `a`, compile the fn (installs bcode), run the same call — now it jumps
        // to the compiled body — and diff the result and the side effect on `a`.
        unsafe { std::ptr::write_unaligned(a_val as *mut i64, 0) };
        // SAFETY: `func`/`a` live in `store`, which outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i64) };

        assert_eq!(interp, 5_000_000_000, "interpreter result");
        assert_eq!(interp_a, 5_000_000_000, "interpreter side effect on a");
        assert_eq!(jit, interp, "jit result != interpreter");
        assert_eq!(jit_a, interp_a, "jit side effect on a != interpreter");
    }

    #[test]
    fn milestone_2_fn_runs_interpreted_and_jit_identically() {
        // Milestone 2: a function run both interpreted and Cranelift-JIT-compiled,
        // results diffed. The interpreter is the oracle. The body `return 40 + 2`
        // yields its i32; here through an explicit `return`, though `return` is
        // optional and a bare trailing expression yields the same value (DESIGN ›A
        // scope's value is what it evaluates to‹; see `fn_body_return_is_optional`).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( return 40 + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };

        // The same `run`, two paths on one node: interpret first (no bcode yet),
        // then compile and run again (jumps to the installed bcode). Both diffed.
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);

        // Interpreted: bcode is null, so `run` walks the body.
        let interp = unsafe { rt.run(call) }.unwrap();
        unsafe {
            let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
            assert!(bcode.is_null());
        }

        // Compile installs the exec@ on `func`; keep the artifact alive for the run.
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, core.types(),func) }.unwrap();
        unsafe {
            let bcode = *((*func).value as *const DyadPtr).add(FN_BCODE);
            assert!(!bcode.is_null()); // bcode installed on the node
        }

        // JIT: the same `run(call)` now jumps to the installed bcode.
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 42);
        assert_eq!(jit, interp); // the compiled path matches the interpreter oracle
    }

    #[test]
    fn rational_decimal_parses_but_is_uncomputable_as_i32() {
        // A decimal is a valid rational literal (it parses), but computing it as an
        // i32 has no exact answer: run and compile both report UncomputableLiteral
        // rather than crashing or silently truncating.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("3.14", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap() // parsing a decimal succeeds
        };
        unsafe {
            assert_eq!((*node).ty, core.rational);
            assert_eq!(rational::mold(node), None); // 157/50 has no exact i32
        }

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the rational literal just parsed.
        assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::UncomputableLiteral));
        // SAFETY: same node; compilation reports the same outcome as the oracle.
        let compiled = unsafe { compile_nullary_i32(&core.lower, core.types(),node) };
        assert!(matches!(compiled, Err(crate::compile::CompileError::UncomputableLiteral)));
    }

    #[test]
    fn whole_valued_rationals_still_compute() {
        // `6.0` reduces to 6/1 and molds to 6 — integer literals are the den==1 case.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let node = {
            let mut p = Parser::new("6.0", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the rational literal just parsed.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), 6);
    }

    #[test]
    fn i32_overflow_matches_between_interpreter_and_jit() {
        // `a + a` on an i32 variable = 2_000_000_000 overflows i32; both tiers must wrap
        // to the same i32 (the interpreter must not widen to i64). A concrete operand is
        // required: two comptime literals would fold to an exact rational, not wrap.
        let expected = i64::from(2_000_000_000i32.wrapping_add(2_000_000_000)); // -294967296
        diff_var_fn(NumType::I32, 2_000_000_000, "fn () -> i32 ( a + a )", expected);
    }

    #[test]
    fn four_param_fn_stays_interpreted_and_refuses_to_compile() {
        // The compiled calling convention supports at most three i32 args, so a
        // 4-param function fails compilation (UnsupportedArity) rather than
        // installing bcode a call cannot invoke; interpreted, it runs fine.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add4 = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (a : i32, b : i32, c : i32, d : i32) -> i32 ( return a + b + c + d )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // Compilation refuses the arity up front.
        // SAFETY: `add4` is the fn node just built.
        let result = unsafe { compile_fn(&core.lower, core.types(),add4) };
        assert!(matches!(result, Err(crate::compile::CompileError::UnsupportedArity(4))));

        // Interpreted, the same function computes (bcode was never installed).
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add4", add4).unwrap();
            let mut p =
                Parser::new("add4(1, 2, 3, 4)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`add4`/args are valid nodes just parsed.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 10);
    }

    #[test]
    fn compiling_an_uninitialized_read_errors_instead_of_crashing() {
        // A declared-but-uninitialised i32 (null storage) compiled would bake a load
        // from address 0 and SIGSEGV; instead compilation errors with BadValue, the
        // same outcome the interpreter reaches.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        // `x`: an i32 variable with no storage yet.
        let x = store.alloc_raw(core.i32_, std::ptr::null_mut());
        scopes.declare(&mut trie, "x", x).unwrap();

        let node = {
            let mut p = Parser::new("x", &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        // Interpreter: clean BadValue.
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the variable reference just parsed.
        assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::BadValue));
        // Compiler: BadValue, not a baked load from address 0.
        // SAFETY: same node; the lowering guards the null storage.
        let compiled = unsafe { compile_nullary_i32(&core.lower, core.types(),node) };
        assert!(matches!(compiled, Err(crate::compile::CompileError::BadValue)));
    }

    #[test]
    fn plus_is_abstract_and_resolves_to_a_concrete_op() {
        // `+` is not itself a machine addition: it stays reflectable (its node's type
        // is still `+`) but resolves to a concrete op it stores in its value, and both
        // run and compile delegate to it. A concrete operand (`a`) keeps it a `+` node —
        // two comptime literals would fold instead. Nested `+` resolves too.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&10i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let func = {
            let mut p = Parser::new(
                "fn () -> i32 ( a + 20 + 12 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        // The body is a `+` node, reflectable as `+`, carrying its resolved op.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.plus);
            assert_eq!(*((*body).value as *const DyadPtr).add(2), core.i32_);
        }

        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 42);
        assert_eq!(jit, interp);
    }

    #[test]
    fn parses_and_runs_bool_literals() {
        // `true`/`false` are `bool`-typed literals: they parse to a `bool` node and
        // both tiers read 1/0. The interpreter's generic data path reads the i32;
        // the `bool` lowering bakes it as a constant.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        for (src, expect) in [("true", 1i64), ("false", 0i64)] {
            let node = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            // SAFETY: `node` is the literal just parsed.
            unsafe {
                assert_eq!((*node).ty, core.bool_);
            }
            let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
            // SAFETY: `node` is a valid `bool` literal.
            assert_eq!(unsafe { rt.run(node) }.unwrap(), expect);
            // SAFETY: same node; the `bool` lowering bakes its constant.
            let compiled = unsafe { compile_nullary_i32(&core.lower, core.types(),node) }.unwrap();
            assert_eq!(unsafe { compiled.call() }, expect);
        }
    }

    /// Parse `src` as a nullary i32 fn body, diff the interpreter against the JIT,
    /// and assert both equal `expect`. The interpreter is the oracle.
    fn diff_nullary_fn(src: &str, expect: i64) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built and outlives the call.
        let _compiled = unsafe { compile_fn(&core.lower, core.types(),func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, expect, "interpreter: {src}");
        assert_eq!(jit, interp, "jit != interpreter: {src}");
    }

    #[test]
    fn subtraction_and_multiplication_match_between_tiers() {
        diff_nullary_fn("fn () -> i32 ( 10 - 3 )", 7);
        diff_nullary_fn("fn () -> i32 ( 2 * 4 )", 8);
        // `*` binds tighter than `+`: 2 + (3 * 4) = 14, not (2 + 3) * 4 = 20.
        diff_nullary_fn("fn () -> i32 ( 2 + 3 * 4 )", 14);
        // `-` shares `+`'s precedence and is left-associative: (10 - 3) - 2 = 5.
        diff_nullary_fn("fn () -> i32 ( 10 - 3 - 2 )", 5);
    }

    #[test]
    fn minus_and_times_resolve_to_concrete_ops() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (a : i32) -> i32 ( a - a * 3 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // Body is `-`(a, *(a, 3)): `-` resolved to sub_i32, its rhs `*` to mul_i32. A
        // concrete operand keeps each a node — two literals would fold.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.minus);
            let bops = (*body).value as *const DyadPtr;
            assert_eq!(*bops.add(2), core.i32_);
            let rhs = *bops.add(1);
            assert_eq!((*rhs).ty, core.times);
            assert_eq!(*((*rhs).value as *const DyadPtr).add(2), core.i32_);
        }
    }

    #[test]
    fn multiplication_overflow_matches_between_interpreter_and_jit() {
        // `a * a` on an i32 variable = 100000 overflows i32; both tiers wrap to the same
        // i32. A concrete operand is required: `100000 * 100000` folds to an exact
        // rational (10^10) that has no i32 and so would not model the wrap.
        let expected = i64::from(100_000i32.wrapping_mul(100_000));
        diff_var_fn(NumType::I32, 100_000, "fn () -> i32 ( a * a )", expected);
    }

    #[test]
    fn less_than_matches_between_tiers() {
        diff_nullary_fn("fn () -> i32 ( 3 < 5 )", 1);
        diff_nullary_fn("fn () -> i32 ( 5 < 3 )", 0);
        diff_nullary_fn("fn () -> i32 ( 5 < 5 )", 0);
        // `<` binds looser than arithmetic: (2 + 3) < (4 * 2) = 5 < 8 = 1.
        diff_nullary_fn("fn () -> i32 ( 2 + 3 < 4 * 2 )", 1);
    }

    #[test]
    fn less_than_is_abstract_and_resolves_to_lt_i32() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (a : i32) -> i32 ( a < 5 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // The body stays reflectable as `<` and records the concrete op it resolved to
        // (a concrete operand keeps it a node; two literals would fold to a bool).
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.lt);
            assert_eq!(*((*body).value as *const DyadPtr).add(2), core.i32_);
        }
    }

    #[test]
    fn comparison_siblings_match_between_tiers() {
        // >, ==, <=, >=, != each yield the i32 0/1, diffed interpreter vs JIT.
        diff_nullary_fn("fn () -> i32 ( 5 > 3 )", 1);
        diff_nullary_fn("fn () -> i32 ( 3 > 5 )", 0);
        diff_nullary_fn("fn () -> i32 ( 5 > 5 )", 0);
        diff_nullary_fn("fn () -> i32 ( 4 == 4 )", 1);
        diff_nullary_fn("fn () -> i32 ( 4 == 5 )", 0);
        diff_nullary_fn("fn () -> i32 ( 3 <= 3 )", 1);
        diff_nullary_fn("fn () -> i32 ( 4 <= 3 )", 0);
        diff_nullary_fn("fn () -> i32 ( 2 <= 3 )", 1);
        diff_nullary_fn("fn () -> i32 ( 3 >= 3 )", 1);
        diff_nullary_fn("fn () -> i32 ( 2 >= 3 )", 0);
        diff_nullary_fn("fn () -> i32 ( 4 >= 3 )", 1);
        diff_nullary_fn("fn () -> i32 ( 4 != 5 )", 1);
        diff_nullary_fn("fn () -> i32 ( 4 != 4 )", 0);
        // Signed comparison, with a computed negative operand.
        diff_nullary_fn("fn () -> i32 ( 0 - 1 < 0 )", 1);
        // Comparisons bind looser than arithmetic: (2+3) == (10-5) = 5 == 5 = 1.
        diff_nullary_fn("fn () -> i32 ( 2 + 3 == 10 - 5 )", 1);
    }

    #[test]
    fn comparison_siblings_are_bool_conditions_for_if() {
        // Each comparison's result is a `bool`, so it is a valid `if` condition.
        diff_nullary_fn("fn () -> i32 ( if (5 > 3) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (2 == 3) (100) else (200) )", 200);
        diff_nullary_fn("fn () -> i32 ( if (3 <= 3) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (3 >= 4) (100) else (200) )", 200);
        diff_nullary_fn("fn () -> i32 ( if (7 != 7) (100) else (200) )", 200);
    }

    #[test]
    fn comparison_siblings_resolve_to_their_concrete_ops() {
        // Each abstract operator stays reflectable and records the concrete op it
        // resolved to at operand index 2 (the trie longest-matches `<=`/`>=`/`==`
        // over `<`/`>`/`=`).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // A concrete operand (`a`) keeps each a node; two literals would fold to a bool.
        let cases: [(&str, DyadPtr, DyadPtr); 5] = [
            ("fn (a : i32) -> i32 ( a > 2 )", core.gt, core.i32_),
            ("fn (a : i32) -> i32 ( a == 2 )", core.eq, core.i32_),
            ("fn (a : i32) -> i32 ( a <= 2 )", core.le, core.i32_),
            ("fn (a : i32) -> i32 ( a >= 2 )", core.ge, core.i32_),
            ("fn (a : i32) -> i32 ( a != 2 )", core.ne, core.i32_),
        ];
        for (src, abstract_op, concrete) in cases {
            let func = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            // SAFETY: `func` is the fn node just parsed.
            unsafe {
                let body = *((*func).value as *const DyadPtr).add(FN_BODY);
                assert_eq!((*body).ty, abstract_op, "abstract op for `{src}`");
                assert_eq!(
                    *((*body).value as *const DyadPtr).add(2),
                    concrete,
                    "concrete op for `{src}`"
                );
            }
        }
    }

    #[test]
    fn logical_operators_match_between_tiers() {
        // `and`/`or` truth tables and `not`, diffed interpreter vs JIT.
        diff_nullary_fn("fn () -> i32 ( true and true )", 1);
        diff_nullary_fn("fn () -> i32 ( true and false )", 0);
        diff_nullary_fn("fn () -> i32 ( false and true )", 0);
        diff_nullary_fn("fn () -> i32 ( false and false )", 0);
        diff_nullary_fn("fn () -> i32 ( true or true )", 1);
        diff_nullary_fn("fn () -> i32 ( true or false )", 1);
        diff_nullary_fn("fn () -> i32 ( false or true )", 1);
        diff_nullary_fn("fn () -> i32 ( false or false )", 0);
        diff_nullary_fn("fn () -> i32 ( not (true) )", 0);
        diff_nullary_fn("fn () -> i32 ( not (false) )", 1);
        // Over comparisons (their natural operands).
        diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 < 4 )", 1);
        diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 )", 0);
        diff_nullary_fn("fn () -> i32 ( 1 > 2 or 3 < 4 )", 1);
        diff_nullary_fn("fn () -> i32 ( not (1 < 2) )", 0);
        // Precedence: comparisons tighter than `and` tighter than `or`, so
        // `1<2 and 3>4 or 5<6` = `((1<2) and (3>4)) or (5<6)` = (T and F) or T = T.
        diff_nullary_fn("fn () -> i32 ( 1 < 2 and 3 > 4 or 5 < 6 )", 1);
        // Logical results are bool, so they nest and serve as `if` conditions.
        diff_nullary_fn("fn () -> i32 ( not (1 < 2) or not (3 < 4) )", 0);
        diff_nullary_fn("fn () -> i32 ( if (1 < 2 and 3 < 4) (100) else (200) )", 100);
    }

    #[test]
    fn logical_operators_short_circuit_on_the_interpreter() {
        // `and`/`or` short-circuit: the right operand is not evaluated when the left
        // decides the result. Observed via a right operand that would error (an
        // uninitialized read) but is skipped.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        // `y`: a declared-but-uninitialized i32; reading it is a BadValue.
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let y = store.alloc_raw(core.i32_, std::ptr::null_mut());
            s.declare(&mut trie, "y", y).unwrap();
        }
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);

        // The right operand alone errors — this is what short-circuiting must skip.
        let bad = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("y < 1", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(unsafe { rt.run(bad) }, Err(crate::run::RunError::BadValue));

        // `false and (y < 1)`: false left operand skips the erroring read → 0.
        let and_sc = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("false and y < 1", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(unsafe { rt.run(and_sc) }.unwrap(), 0);

        // `true or (y < 1)`: true left operand skips the erroring read → 1.
        let or_sc = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("true or y < 1", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(unsafe { rt.run(or_sc) }.unwrap(), 1);
    }

    #[test]
    fn logical_operators_reject_non_bool_operands() {
        // `and`/`or`/`not` require `bool` operands; a number is not one.
        for src in ["true and 1", "1 or false", "not (1)"] {
            let mut store = Store::new();
            let mut trie = RegexTrie::new();
            let core = Core::build(&mut store, &mut trie);
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
            assert_eq!(
                p.parse_expression(),
                Err(crate::parse::ParseError::NonBoolOperands),
                "`{src}` should be rejected",
            );
        }
    }

    #[test]
    fn not_requires_a_parenthesized_operand() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("not true", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::ExpectedOpen));
    }

    #[test]
    fn if_with_bool_literal_conditions_match_between_tiers() {
        // A `bool`-literal condition selects a branch; both tiers agree.
        diff_nullary_fn("fn () -> i32 ( if (true) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (false) (100) else (200) )", 200);
        // A comparison condition, nullary so both operands are constants.
        diff_nullary_fn("fn () -> i32 ( if (0 < 1) (100) else (200) )", 100);
        diff_nullary_fn("fn () -> i32 ( if (1 < 0) (100) else (200) )", 200);
    }

    #[test]
    fn if_over_a_parameter_matches_between_tiers() {
        // The non-recursive control-flow shape: a parameterized `if` fn, each branch
        // diffed interpreted vs JIT. n=0 takes the then-branch, n=5 the else.
        for (arg, expect) in [(0i64, 100i64), (5, 200)] {
            let mut store = Store::new();
            let mut trie = RegexTrie::new();
            let core = Core::build(&mut store, &mut trie);

            let func = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let mut p = Parser::new(
                    "fn (n : i32) -> i32 ( if (n < 1) (100) else (200) )",
                    &mut store,
                    &mut trie,
                    &core.metas,
                    core.types(),
                    s,
                );
                p.parse_expression().unwrap()
            };
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                s.declare(&mut trie, "f", func).unwrap();
                let src = format!("f({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
            // SAFETY: `call`/`func`/body are valid nodes just parsed.
            let interp = unsafe { rt.run(call) }.unwrap();
            // SAFETY: `func` is the fn node just built and outlives the call.
            let _compiled = unsafe { compile_fn(&core.lower, core.types(),func) }.unwrap();
            let jit = unsafe { rt.run(call) }.unwrap();
            assert_eq!(interp, expect, "interpreter n={arg}");
            assert_eq!(jit, interp, "jit != interpreter n={arg}");
        }
    }

    #[test]
    fn if_with_a_non_bool_condition_is_rejected() {
        // The condition must be a `bool`; a bare number is not one.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);

        let mut p = Parser::new(
            "fn () -> i32 ( if (1) (100) else (200) )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            s,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::NonBoolCondition));
    }

    #[test]
    fn nested_if_matches_between_tiers() {
        // A then-branch that is itself an `if`, exercising nested merge blocks.
        diff_nullary_fn(
            "fn () -> i32 ( if (true) ( if (false) (1) else (2) ) else (3) )",
            2,
        );
    }

    #[test]
    fn else_less_if_is_a_unit_statement_both_tiers() {
        // An else-less `if` runs its then-branch for its effect when taken and
        // yields unit either way. From a = 41 the branch is taken (41 < 100) and
        // bumps a to 42; from a = 100 it is not, and a stays. Both tiers diffed on
        // the unit result and the effect.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();
        let func = {
            let mut p = Parser::new(
                "fn () -> void ( if (a < 100) (a = a + 1) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Interpreted: taken bumps a to 42; from 100, not taken, a stays.
        // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0, "unit (interpreted)");
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 42);
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 100) };
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 100);
        // Compiled: the same effect and unit on both paths.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 41) };
        // SAFETY: `func`/`a` live in `store`, which outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0, "unit (compiled)");
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 42);
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 100) };
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 100);
    }

    #[test]
    fn else_less_if_in_a_void_fn_yields_unit_both_tiers() {
        // Taken or not, an else-less `if` yields unit through a `-> void` fn.
        diff_typed_call("fn (c : i32) -> void ( if (c < 1) (42) )", "f(0)", 0);
        diff_typed_call("fn (c : i32) -> void ( if (c < 1) (42) )", "f(5)", 0);
    }

    #[test]
    fn an_else_less_if_is_rejected_in_value_positions() {
        // A numeric fn tail needs a value on both paths, so an else-less `if`
        // cannot be one; nor can it feed an operator.
        assert_eq!(parse_err("fn () -> i32 ( if (1 < 2) (1) )"), ParseError::MissingElse);
        assert_eq!(parse_err("( if (1 < 2) (1) ) + 1"), ParseError::UnsupportedOperands);
    }

    #[test]
    fn the_else_binds_to_the_outer_if_across_a_bracketed_branch() {
        // The dangling-else question: the inner `if` is else-less inside its
        // bracketed branch, so the bracket ends its reach and the `else` belongs to
        // the outer `if`. With a = 5 the outer condition is false and the
        // else-branch must run (a becomes 7); were the `else` the inner if's,
        // nothing would run.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&5i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();
        let func = {
            let mut p = Parser::new(
                "fn () -> void ( if (a < 1) ( if (a < 1) (a = a + 1) ) else (a = a + 2) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Interpreted: outer condition false -> else-branch -> a = 7.
        // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
        // Compiled: reset and diff the same effect.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 5) };
        // SAFETY: `func`/`a` live in `store`, which outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        assert_eq!(unsafe { rt.run(call) }.unwrap(), 0);
        assert_eq!(unsafe { std::ptr::read_unaligned(a_val as *const i32) }, 7);
    }

    #[test]
    fn assignment_commits_a_literal_to_the_targets_type() {
        // The rhs literal commits to the variable's declared type at parse time, so
        // an i64 target takes a value past i32 exactly, in both tiers.
        diff_var_fn(NumType::I64, 0, "fn () -> i64 ( a = 5000000000  a )", 5_000_000_000);
        // And a literal with no exact value in the target is a parse error.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let a = store.alloc_raw(core.i32_, std::ptr::null_mut());
        s.declare(&mut trie, "a", a).unwrap();
        let mut p = Parser::new("a = 3.5", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
    }

    #[test]
    fn compiled_calls_between_compiled_functions_are_width_general() {
        // The outer's compiled call passes i64 containers and reads back an i64
        // result: 2e9 * 3 = 6e9 crosses i32, so an i32-assuming boundary would
        // truncate. Oracle: outer interpreted over the compiled callee.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let mul = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i64, y : i64) -> i64 ( x * y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let outer = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "mul", mul).unwrap();
            let mut p = Parser::new(
                "fn () -> i64 ( mul(2000000000, 3) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(outer, std::ptr::null_mut());

        // SAFETY: `mul` is the fn node just built and outlives every call.
        let _c_mul = unsafe { compile_fn(&core.lower, core.types(), mul) }.unwrap();
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`outer` are valid nodes; the callee's artifact is alive.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `outer` is the fn node just built; both artifacts stay alive.
        let _c_outer = unsafe { compile_fn(&core.lower, core.types(), outer) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 6_000_000_000, "interpreter over compiled callee");
        assert_eq!(jit, interp, "fully compiled chain != oracle");
    }

    #[test]
    fn compiled_calls_pass_floats_across_the_boundary() {
        // An f64 argument rides the container as raw bits (a bitcast, not an
        // extend), so a float callee is the other ABI shape worth its own diff.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&2.5f64.to_bits().to_ne_bytes());
        let a = store.alloc_raw(core.numtypes[NumType::F64 as usize], a_val);
        scopes.declare(&mut trie, "a", a).unwrap();

        let g = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : f64) -> f64 ( x + 0.5 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        scopes.declare(&mut trie, "g", g).unwrap();
        let outer = {
            let mut p = Parser::new(
                "fn () -> f64 ( g(a) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(outer, std::ptr::null_mut());

        // SAFETY: `g` is the fn node just built and outlives every call.
        let _c_g = unsafe { compile_fn(&core.lower, core.types(), g) }.unwrap();
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`outer`/`a` are valid nodes; the callee's artifact is alive.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `outer` is the fn node just built; both artifacts stay alive.
        let _c_outer = unsafe { compile_fn(&core.lower, core.types(), outer) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 3.0f64.to_bits() as i64, "interpreter over compiled callee");
        assert_eq!(jit, interp, "fully compiled chain != oracle");
    }

    #[test]
    fn compiled_self_recursion_is_width_general() {
        // A recursive i64 fn compiled whole: the self-call passes and returns i64
        // containers, and the base case alone exceeds i32.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let s_fn = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "s := fn (n : i64) -> i64 ( if (n < 1) (2000000000 + 2000000000) else (s(n - 1)) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("s(3)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Oracle: interpret the recursion (bcode not installed yet).
        // SAFETY: `call` applies the bound `s` to a literal.
        let interp = unsafe { rt.run(call) }.unwrap();
        // Compile (installs bcode; the artifact must outlive the compiled call).
        // SAFETY: `s_fn` is the fn node just built and outlives every call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), s_fn) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 4_000_000_000, "interpreter");
        assert_eq!(jit, interp, "compiled recursion != interpreter");
    }

    #[test]
    fn compiled_call_with_wrong_arity_refuses_to_compile() {
        // `add(40)` for a two-parameter callee: the interpreter errors at run time;
        // compiling the caller refuses up front instead of baking a bad call.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // SAFETY: `add` is the fn node just built.
        let _c_add = unsafe { compile_fn(&core.lower, core.types(), add) }.unwrap();
        let outer = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p = Parser::new(
                "fn () -> i32 ( add(40) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // SAFETY: `outer` is the fn node just built.
        let result = unsafe { compile_fn(&core.lower, core.types(), outer) };
        assert!(matches!(result, Err(crate::compile::CompileError::ArityMismatch)));
    }

    #[test]
    fn call_arguments_commit_to_parameter_types() {
        // An argument literal lands in the parameter's typed slot: an i64 parameter
        // takes a value past i32 exactly, and a float parameter takes a decimal —
        // neither squeezes through an i32 default.
        diff_typed_call("fn (x : i64) -> i64 ( x )", "f(3000000000)", 3_000_000_000);
        diff_typed_call("fn (x : f64) -> f64 ( x + 0.5 )", "f(2.5)", 3.0f64.to_bits() as i64);
    }

    #[test]
    fn an_argument_that_does_not_fit_its_parameter_is_rejected() {
        // A decimal into an integer parameter has no exact value: a parse error at
        // the call site, not a runtime surprise.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32) -> i32 ( x )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        s.declare(&mut trie, "f", func).unwrap();
        let mut p = Parser::new("f(2.5)", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(p.parse_expression(), Err(ParseError::UncomputableLiteral));
    }

    #[test]
    fn recursive_i64_factorial_matches_between_tiers() {
        // The published-signature hint: inside `n * fact(n - 1)` the self-call must
        // read i64 from the declaration's signature (an unbound placeholder would
        // default to i32 and mismatch `n`). 20! = 2.4e18 needs the width.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let fact = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fact := fn (n : i64) -> i64 ( if (n < 1) (1) else (n * fact(n - 1)) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("fact(20)", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call` applies the bound `fact` to a literal.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `fact` outlives every call; the artifact stays alive for the run.
        let _c = unsafe { compile_fn(&core.lower, core.types(), fact) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 2_432_902_008_176_640_000, "interpreter 20!");
        assert_eq!(jit, interp, "compiled 20! != interpreter");
    }

    #[test]
    fn hash_comments_are_trivia() {
        // `#` runs to the end of its line, as in the sketch's own files; comments
        // weave through a sequence without separating anything themselves.
        diff_nullary_fn("fn () -> i32 ( # the answer\n a := 40 # forty\n a + 2 )", 42);
    }

    #[test]
    fn while_loop_sums_both_tiers() {
        // The loop shape: block-local typed variables (the sketch's `sum := i32 0`
        // juxtaposition, with real storage), a while statement mutating them, the
        // trailing read. The explicit re-zeroing after the declarations makes the
        // body idempotent (`:=` initializes at parse, not per run), so the
        // interpreted run and the compiled rerun agree from any prior state.
        diff_nullary_fn(
            "fn () -> i32 ( a := i32 0  i := i32 0  a = 0  i = 0  while (i < 5) ( a = a + i  i = i + 1 )  a )",
            10,
        );
    }

    #[test]
    fn while_false_never_runs_its_body() {
        diff_nullary_fn("fn () -> i32 ( a := i32 7  a = 7  while (a < 0) (a = 0)  a )", 7);
    }

    #[test]
    fn for_loop_sums_a_range_both_tiers() {
        // `for i in 0..10` is end-exclusive (0 through 9, per the old prototype's
        // ((end-start)/delta).ceil() count): sum = 45. The explicit re-zeroing
        // keeps the body idempotent across the interpreted and compiled runs.
        diff_nullary_fn(
            "fn () -> i32 ( s := i32 0  s = 0  for i in 0..10 ( s = s + i )  s )",
            45,
        );
        // With a step: 0, 2, 4, 6, 8 sum to 20.
        diff_nullary_fn(
            "fn () -> i32 ( s := i32 0  s = 0  for i in 0..10..2 ( s = s + i )  s )",
            20,
        );
        // An empty range runs zero iterations.
        diff_nullary_fn(
            "fn () -> i32 ( s := i32 7  s = 7  for i in 5..5 ( s = 0 )  s )",
            7,
        );
    }

    #[test]
    fn for_loop_endpoints_resolve_a_common_type() {
        // i64 endpoints past i32's range: the loop variable is i64 and the sum
        // crosses i32. 5e9..5e9+3 sums to 15e9 + 3.
        diff_typed_call(
            "fn (n : i64) -> i64 ( s := i64 0  s = 0  for i in n..(n + 3) ( s = s + i )  s )",
            "f(5000000000)",
            15_000_000_003,
        );
        // Mismatched concrete endpoint types are rejected.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (a : i32, b : i64) -> void ( for i in a..b ( a = 0 ) )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            s,
        );
        assert_eq!(p.parse_expression(), Err(ParseError::TypeMismatch));
    }

    #[test]
    fn for_loop_with_a_runtime_non_positive_step_runs_zero_iterations() {
        // The step guard both tiers emit: a runtime step of 0 (or negative) skips
        // the loop instead of wrapping forever.
        diff_typed_call(
            "fn (d : i32) -> i32 ( s := i32 3  s = 3  for i in 0..10..d ( s = 0 )  s )",
            "f(0)",
            3,
        );
    }

    #[test]
    fn for_loop_shapes_are_checked() {
        // A literal non-positive step; a for as a value; a return in the body;
        // missing `in`; a non-primary endpoint.
        assert_eq!(parse_err("for i in 0..10..0 ( 1 )"), ParseError::BadStep);
        assert_eq!(parse_err("for i in 10..0..-1 ( 1 )"), ParseError::BadStep);
        assert_eq!(
            parse_err("fn () -> i32 ( for i in 0..3 ( 1 ) )"),
            ParseError::StatementAsValue
        );
        assert_eq!(
            parse_err("fn () -> void ( for i in 0..3 ( return 1 ) )"),
            ParseError::EarlyReturn
        );
        assert_eq!(parse_err("for i 0..3 ( 1 )"), ParseError::ExpectedIn);
        assert_eq!(parse_err("for i in 0 ( 1 )"), ParseError::ExpectedRange);
    }

    #[test]
    fn while_condition_must_be_bool() {
        assert_eq!(parse_err("while (1) (2)"), ParseError::NonBoolCondition);
    }

    #[test]
    fn a_while_loop_is_not_a_value() {
        assert_eq!(parse_err("fn () -> i32 ( while (1 < 2) (3) )"), ParseError::StatementAsValue);
        assert_eq!(parse_err("( while (1 < 2) (3) ) + 1"), ParseError::UnsupportedOperands);
    }

    #[test]
    fn a_return_inside_a_while_body_is_rejected() {
        // v1 has no unwinding to exit a loop with; running the return without
        // exiting would be silently wrong.
        assert_eq!(
            parse_err("fn () -> void ( while (1 < 2) (return 1) )"),
            ParseError::EarlyReturn
        );
    }

    #[test]
    fn juxtaposition_types_a_literal() {
        // `i64 5000000000` is the anonymous typed value (DESIGN ›written by
        // juxtaposition‹): the literal commits exactly to the type before it.
        diff_typed_call("fn () -> i64 ( x := i64 5000000000  x )", "f()", 5_000_000_000);
        // An exact commit, not a wrapping cast: a decimal into i32 is an error.
        assert_eq!(parse_err("i32 3.5"), ParseError::UncomputableLiteral);
        // The settled separator doctrine: `f(i32 3)` is ONE argument (the typed
        // value), where `f(i32, 3)` would be two.
        diff_typed_call("fn (x : i32) -> i32 ( x + 1 )", "f(i32 3)", 4);
    }

    #[test]
    fn division_and_remainder_match_between_tiers() {
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x / y )", "f(10, 3)", 3);
        // Truncates toward zero, matching the interpreter and Rust.
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x / y )", "f(-10, 3)", -3);
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x % y )", "f(10, 3)", 1);
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x % y )", "f(-10, 3)", -1);
        // `/` binds like `*`: 10 - 4/2 = 8 (the literal quotient folds exactly).
        diff_typed_call("fn (x : i32) -> i32 ( x - 4 / 2 )", "f(10)", 8);
    }

    #[test]
    fn division_by_zero_saturates_to_max_both_tiers() {
        // Settled: a zero divisor yields the type's MAX — a loud sentinel, easier
        // to discover than 0 — and signed MIN/-1, the other impossible quotient,
        // saturates to MAX too. MIN % -1 is the well-defined 0.
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x / y )", "f(10, 0)", i64::from(i32::MAX));
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x % y )", "f(10, 0)", i64::from(i32::MAX));
        diff_typed_call("fn (x : u8, y : u8) -> u8 ( x / y )", "f(7, 0)", 255);
        diff_typed_call(
            "fn (x : i32, y : i32) -> i32 ( x / y )",
            "f(-2147483648, -1)",
            i64::from(i32::MAX),
        );
        diff_typed_call("fn (x : i32, y : i32) -> i32 ( x % y )", "f(-2147483648, -1)", 0);
    }

    #[test]
    fn float_division_is_ieee_and_float_remainder_is_rejected() {
        diff_typed_call(
            "fn (x : f64, y : f64) -> f64 ( x / y )",
            "f(1.0, 2.0)",
            0.5f64.to_bits() as i64,
        );
        // IEEE: x / 0.0 is inf, in both tiers, no sentinel needed.
        diff_typed_call(
            "fn (x : f64, y : f64) -> f64 ( x / y )",
            "f(1.0, 0.0)",
            f64::INFINITY.to_bits() as i64,
        );
        // No Cranelift float remainder; `%` over floats is rejected at parse.
        assert_eq!(parse_err("fn (x : f64) -> f64 ( x % 2.0 )"), ParseError::UnsupportedOperands);
    }

    #[test]
    fn comptime_division_is_exact_fractions() {
        // 1/3 is an exact fraction at comptime, so (1/3)*3 is exactly 1 — no
        // float could say that — and explicit truncation is the cast.
        diff_typed_call("fn () -> f64 ( f64(1 / 3 * 3) )", "f()", 1.0f64.to_bits() as i64);
        diff_typed_call("fn () -> i32 ( i32(10 / 3) )", "f()", 3);
        diff_typed_call("fn () -> i32 ( 10 % 3 )", "f()", 1);
        // A literal zero divisor has no comptime value.
        assert_eq!(parse_err("i32(1 / 0)"), ParseError::UncomputableLiteral);
        assert_eq!(parse_err("i32(1 % 0)"), ParseError::UncomputableLiteral);
    }

    #[test]
    fn string_literals_parse_and_are_inert() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let node = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("«hello world»", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `node` is the string literal just parsed.
        unsafe {
            assert_eq!((*node).ty, core.string_);
            assert_eq!(crate::identities::string::text(node), b"hello world");
        }
        // Inert: no scalar to read, and no operator accepts it.
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the string literal just parsed.
        assert_eq!(unsafe { rt.run(node) }, Err(crate::run::RunError::BadValue));
        assert_eq!(parse_err("«a» + 1"), ParseError::UnsupportedOperands);
    }

    #[test]
    fn comments_are_reflectable_nodes_invisible_to_value_flow() {
        // A statement-level `#` builds a comment node: real graph structure whose
        // substance is a string node (both the raw-line and `«…»` forms), never
        // run, never a scope's tail — the 40 + 2 stays the value in both tiers.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( # the answer\n 40 + 2\n # «checked twice» )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        // The body is the sequence [comment, 42, comment]; the tail for typing
        // and value is the 42, committed through the trailing prose.
        unsafe {
            let body = *((*func).value as *const DyadPtr).add(FN_BODY);
            assert_eq!((*body).ty, core.scope_);
            let ops = (*body).value as *const DyadPtr;
            let (c1, mid, c2) = (*ops, *ops.add(1), *ops.add(2));
            assert!((*ops.add(3)).is_null());
            assert_eq!((*c1).ty, core.comment_);
            assert_eq!(crate::identities::string::text((*c1).value.cast()), b"the answer");
            assert_eq!((*mid).ty, core.i32_);
            assert_eq!((*c2).ty, core.comment_);
            assert_eq!(crate::identities::string::text((*c2).value.cast()), b"checked twice");
        }
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/body are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` outlives the call; the artifact stays alive.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 42);
        assert_eq!(jit, interp);
    }

    #[test]
    fn a_scope_of_only_prose_has_no_value() {
        assert_eq!(parse_err("( # just a note )"), ParseError::Empty);
    }

    #[test]
    fn pointers_mutate_caller_state_through_calls_both_tiers() {
        // The headline: a pointer parameter (@i32), the address of a caller
        // variable, and a store-through — the callee mutates the caller's x,
        // interpreted and fully compiled.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "incr := fn (p : @i32) -> void ( p@ = p@ + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }
        let incr = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("incr", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( x := i32 41  x = 41  incr(&x)  x )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/`incr` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // Compile the callee first (the caller's call bakes its address).
        // SAFETY: both fn nodes outlive the calls; the artifacts stay alive.
        let _c_incr = unsafe { compile_fn(&core.lower, core.types(), incr) }.unwrap();
        let _c_func = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 42);
        assert_eq!(jit, interp);
    }

    #[test]
    fn pointer_variables_rewire_both_tiers() {
        // p := &x aliases x; p = &y rewires; p@ reads through the current target.
        // The explicit `p = &x` makes the body idempotent (declarations
        // initialize at parse, and the interpreted run leaves p on y).
        diff_nullary_fn(
            "fn () -> i32 ( x := i32 10  y := i32 20  x = 10  y = 20  p := &x  p = &x  s := i32 0  s = p@  p = &y  s + p@ )",
            30,
        );
    }

    #[test]
    fn pointer_chains_and_field_pointers_work_both_tiers() {
        // A struct pointer with q@.x (the postfix-deref ergonomics the syntax was
        // chosen for), a field pointer &pt.y, and double indirection pp@@.x.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        declare_point(&mut store, &mut trie, &core);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( pt := point(3, 4)  q := &pt  q@.x = q@.x + 10  fp := &pt.y  fp@ = fp@ + 1  pp := &q  pp@@.x + pt.y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // x: 3 + 10 = 13 (via q@.x); y: 4 + 1 = 5 (via fp@); 13 + 5 = 18.
        // SAFETY: `call`/`func` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` outlives the call; the artifact stays alive.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 18);
        assert_eq!(jit, interp);
    }

    #[test]
    fn struct_pointer_fields_hold_addresses_both_tiers() {
        // A pointer-typed field (@i32): constructed from &x, read through h.r@.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "holder := struct (r : @i32)",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( x := i32 7  x = 7  h := holder(&x)  h.r@ + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` outlives the call; the artifact stays alive.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 8);
        assert_eq!(jit, interp);
    }

    #[test]
    fn pointer_misuse_is_rejected() {
        // Deref of a non-pointer; pointer arithmetic; a literal into a pointer
        // variable; & of a comptime binding; & of a parameter (no storage); a
        // pointer as a numeric function's tail.
        assert_eq!(parse_err("( x := i32 1  x@ )"), ParseError::UnsupportedOperands);
        assert_eq!(
            parse_err("( x := i32 1  p := &x  p + 1 )"),
            ParseError::UnsupportedOperands
        );
        assert_eq!(parse_err("( x := i32 1  p := &x  p = 5 )"), ParseError::TypeMismatch);
        assert_eq!(parse_err("( y := 5  &y )"), ParseError::BadAddressOf);
        assert_eq!(parse_err("fn (a : i32) -> void ( q := &a  q@ )"), ParseError::BadAddressOf);
        assert_eq!(parse_err("fn () -> i32 ( x := i32 1  x = 1  &x )"), ParseError::TypeMismatch);
    }

    #[test]
    fn a_literal_into_a_pointer_parameter_is_rejected() {
        // f(0) against p : @i32 would dereference address 0; the call site
        // rejects it instead.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "f := fn (p : @i32) -> void ( p@ = 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("f(0)", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(p.parse_expression(), Err(ParseError::TypeMismatch));
    }

    /// Declare `point := struct (x : i32, y : i32)` in the root scope.
    fn declare_point(store: &mut Store, trie: &mut RegexTrie, core: &Core) {
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "point := struct (x : i32, y : i32)",
            store,
            trie,
            &core.metas,
            core.types(),
            s,
        );
        p.parse_expression().unwrap();
    }

    #[test]
    fn struct_instances_construct_read_and_write_fields_both_tiers() {
        // The type applied to its field values constructs the instance
        // (point(3, 4), the type-constructor doctrine); `.` resolves to a place
        // inside the instance's storage, so reads and writes are ordinary numeric
        // paths. Construction re-runs per call, so both tiers start from (3, 4).
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        declare_point(&mut store, &mut trie, &core);

        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn () -> i32 ( p := point(3, 4)  p.x = p.x + 36  p.x + p.y + 2 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // 3 + 36 = 39; 39 + 4 + 2 = 45.
        // SAFETY: `call`/`func` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` outlives the call; the artifact stays alive.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 45);
        assert_eq!(jit, interp);
    }

    #[test]
    fn struct_fields_lay_out_mixed_widths() {
        // u8, i64, i32 pack in declaration order (offsets 0, 1, 9); a runtime
        // argument reaches its field, and each field reads back at its own width.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "cell := struct (a : u8, b : i64, c : i32)",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (n : i64) -> i64 ( q := cell(200, n, 7)  q.b + i64(q.a) + i64(q.c) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "f", func).unwrap();
            let mut p = Parser::new(
                "f(5000000000)",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` outlives the call; the artifact stays alive.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, 5_000_000_207);
        assert_eq!(jit, interp);
    }

    #[test]
    fn construction_and_field_access_are_checked() {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        declare_point(&mut store, &mut trie, &core);

        let mut check = |src: &str, expect: ParseError| {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
            assert_eq!(p.parse_expression(), Err(expect), "`{src}`");
        };
        // Wrong argument count; a literal with no exact field value; an instance
        // used as a value; a construction as a numeric tail; an unknown field.
        check("point(1)", ParseError::CtorArity);
        check("point(1.5, 2)", ParseError::UncomputableLiteral);
        check("point(1, 2) + 1", ParseError::UnsupportedOperands);
        check("fn () -> i32 ( p := point(1, 2) )", ParseError::StatementAsValue);
        check(
            "( p := point(1, 2)  p.z )",
            ParseError::Resolve(crate::parse::ResolveError::Unknown),
        );
    }

    #[test]
    fn assigning_into_a_comptime_binding_is_rejected() {
        // `x := 5` binds a comptime rational (no machine storage); writing its
        // value slot would corrupt the fraction, so `=` demands a typed variable.
        assert_eq!(parse_err("( x := 5  x = 7 )"), ParseError::BadAssignTarget);
    }

    #[test]
    fn sequences_run_in_order_and_yield_the_trailing_expression() {
        // A scope's body is a sequence of self-delimiting expressions with no
        // separator; its value is the trailing one. The body assigns first, so the
        // interpreted and compiled runs are both deterministic from any start.
        diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10  a = a + 1  a + 1 )", 12);
    }

    #[test]
    fn the_comma_is_an_optional_readability_separator() {
        // The same sequence with `,` written between the expressions: the comma
        // marks a boundary the expressions already imply.
        diff_var_fn(NumType::I32, 0, "fn () -> i32 ( a = 10, a = a + 1, a + 1 )", 12);
    }

    #[test]
    fn block_local_declarations_do_not_leak() {
        // `( x := 5, x + 1 )` declares `x` in the block's own scope: the block
        // computes with it (both literals fold to 6), and after the block closes
        // the name is a genuine out-of-scope use, not an unknown one.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let node = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("( x := 5, x + 1 )", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `node` is the sequence just parsed.
        assert_eq!(unsafe { rt.run(node) }.unwrap(), 6);
        // SAFETY: same node; the sequence lowering yields its trailing value.
        let compiled = unsafe { compile_nullary_i32(&core.lower, core.types(), node) }.unwrap();
        assert_eq!(unsafe { compiled.call() }, 6);

        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("x", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(
            p.parse_expression(),
            Err(crate::parse::ParseError::Resolve(crate::parse::ResolveError::OutOfScope))
        );
    }

    #[test]
    fn an_early_return_in_a_sequence_is_rejected() {
        // v1 `return` is the tail yield; running one mid-sequence without exiting
        // would be silently wrong, so it is rejected — directly or inside an `if`.
        assert_eq!(parse_err("( return 1, 2 )"), ParseError::EarlyReturn);
        assert_eq!(
            parse_err("fn () -> i32 ( if (true) (return 1) else (0), 2 )"),
            ParseError::EarlyReturn
        );
    }

    #[test]
    fn a_tail_return_in_a_sequence_still_yields() {
        diff_nullary_fn("fn () -> i32 ( 1 + 1, return 40 + 2 )", 42);
    }

    #[test]
    fn adjacent_minus_is_subtraction() {
        // The literal regex is unsigned, so `a-1` lexes as `a`, `-`, `1` —
        // subtraction — never as the statement `a` followed by the literal `-1`.
        diff_var_fn(NumType::I32, 43, "fn () -> i32 ( a-1 )", 42);
    }

    #[test]
    fn negative_literals_via_prefix_minus() {
        // A `-` with no left operand negates the following numeric literal at
        // parse time: as an argument, under a cast, and doubled (0 - -3 = 3).
        diff_typed_call("fn (x : i32) -> i32 ( x )", "f(-1)", -1);
        diff_nullary_fn("fn () -> i32 ( i32(-5) )", -5);
        diff_nullary_fn("fn () -> i32 ( 0 - -3 )", 3);
    }

    #[test]
    fn declaration_binds_a_name_to_a_value() {
        // `x := 5` binds `x` (declared before its value is parsed); the expression's
        // value is the bound node, and a later `x` resolves to that same node.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let decl = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("x := 5", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        // The placeholder became the value: a rational that molds to 5.
        unsafe {
            assert_eq!((*decl).ty, core.rational);
            assert_eq!(rational::mold(decl), Some(5));
        }

        let x_ref = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new("x", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        assert_eq!(x_ref, decl); // the reference resolves to the bound node
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `x_ref` is the bound rational node.
        assert_eq!(unsafe { rt.run(x_ref) }.unwrap(), 5);
    }

    #[test]
    fn redeclaration_in_the_same_scope_is_rejected() {
        // `:=` reuses the no-shadowing check: redeclaring a live name is an error.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p =
                Parser::new("y := 1", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap();
        }
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new("y := 2", &mut store, &mut trie, &core.metas, core.types(), s);
        assert_eq!(
            p.parse_expression(),
            Err(crate::parse::ParseError::Resolve(crate::parse::ResolveError::Shadowed))
        );
    }

    #[test]
    fn interpreted_recursive_factorial() {
        // The payoff: a recursive function on the interpreter. `fact` names itself
        // via `:=` (declared before its body is parsed), the body branches on `if`,
        // and each call runs on its own parameter frame. `n * fact(n - 1)` resolves
        // `*` while `fact` is still an unbound placeholder — the fn-typed placeholder
        // is what lets the self-call read as numeric.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fact := fn (n : i32) -> i32 ( if (n < 1) (1) else (n * fact(n - 1)) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap();
        }

        for (arg, expect) in [(0i64, 1i64), (1, 1), (5, 120)] {
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let src = format!("fact({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
            // SAFETY: `call` applies the bound `fact` to a literal.
            assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "fact({arg})");
        }
    }

    #[test]
    fn compiled_recursive_factorial_matches_the_interpreter() {
        // Milestone 3: a recursive function on *both* tiers. Compiling `fact` turns
        // its body's `fact(n-1)` into a direct machine `call` to itself, so the whole
        // recursion runs in compiled code. Diffed against the interpreter oracle.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        // Parsing the definition returns the bound `fact` node (the fn).
        let fact = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fact := fn (n : i32) -> i32 ( if (n < 1) (1) else (n * fact(n - 1)) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let cases = [(0i64, 1i64), (1, 1), (5, 120), (7, 5040)];

        // Oracle: interpret each call (bcode not yet installed → body walk).
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        for &(arg, expect) in &cases {
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let src = format!("fact({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            // SAFETY: `call` applies the bound `fact` to a literal.
            assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "interpreter fact({arg})");
        }

        // Compile `fact` once; the self-call is installed as a machine call.
        // SAFETY: `fact` is the fn node just built and outlives every call.
        let _compiled = unsafe { compile_fn(&core.lower, core.types(), fact) }.unwrap();
        // SAFETY: reading the installed bcode slot of the fn node.
        unsafe {
            let bcode = *((*fact).value as *const DyadPtr).add(FN_BCODE);
            assert!(!bcode.is_null(), "bcode installed");
        }

        // JIT: the same calls now dispatch to compiled code, which recurses natively.
        for &(arg, expect) in &cases {
            let call = {
                let mut s = ScopeStack::new();
                s.push(core.root_scope);
                let src = format!("fact({arg})");
                let mut p =
                    Parser::new(&src, &mut store, &mut trie, &core.metas, core.types(), s);
                p.parse_expression().unwrap()
            };
            // SAFETY: `_compiled` is alive; `call` applies the compiled `fact`.
            assert_eq!(unsafe { rt.run(call) }.unwrap(), expect, "jit fact({arg})");
        }
    }

    #[test]
    fn compiled_function_calls_another_compiled_function() {
        // A compiled body that calls another already-compiled function, via a machine
        // `call_indirect` to the callee's baked address. Diffed against the oracle.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);

        let add = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(
                "fn (x : i32, y : i32) -> i32 ( x + y )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };

        let outer = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "add", add).unwrap();
            let mut p = Parser::new(
                "fn () -> i32 ( add(40, 2) )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                s,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(outer, std::ptr::null_mut());

        // Compile `add` first so `outer`'s call has a machine address to bake.
        // SAFETY: `add` is the fn node just built and outlives every call.
        let _compiled_add = unsafe { compile_fn(&core.lower, core.types(), add) }.unwrap();

        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Oracle: interpret `outer` (its body calls the already-compiled `add`).
        // SAFETY: `call`/`outer`/`add` are valid nodes; `_compiled_add` is alive.
        let interp = unsafe { rt.run(call) }.unwrap();

        // Compile `outer`: `add(40, 2)` becomes a call_indirect to `add`'s address.
        // SAFETY: `outer` is the fn node just built; both compiled artifacts are alive.
        let _compiled_outer = unsafe { compile_fn(&core.lower, core.types(), outer) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();

        assert_eq!(interp, 42);
        assert_eq!(jit, interp);
    }

    /// Parse a typed function `fn_src`, declare it `f`, parse a call `call_src`, and
    /// diff the interpreter against the JIT, asserting both equal `expect`.
    fn diff_typed_call(fn_src: &str, call_src: &str, expect: i64) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let func = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            let mut p = Parser::new(fn_src, &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let call = {
            let mut s = ScopeStack::new();
            s.push(core.root_scope);
            s.declare(&mut trie, "f", func).unwrap();
            let mut p = Parser::new(call_src, &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func` are valid nodes just parsed.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func` is the fn node just built; the artifact outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, expect, "interpreter: {fn_src} / {call_src}");
        assert_eq!(jit, interp, "jit != interpreter: {fn_src} / {call_src}");
    }

    #[test]
    fn integer_width_arithmetic_matches_between_tiers() {
        // i64 multiplication that overflows i32 (10^10) but fits i64 — proves the width.
        diff_typed_call("fn (x : i64, y : i64) -> i64 ( x * y )", "f(100000, 100000)", 10_000_000_000);
        diff_typed_call("fn (x : i64, y : i64) -> i64 ( x + y )", "f(1000000, 2000000)", 3_000_000);
        // u8 addition wraps at 256: 200 + 100 = 44.
        diff_typed_call("fn (x : u8, y : u8) -> u8 ( x + y )", "f(200, 100)", 44);
        // i16 subtraction, signed negative result.
        diff_typed_call("fn (x : i16, y : i16) -> i16 ( x - y )", "f(3, 10)", -7);
        // u32 sum above i32's range (3e9) stays positive (zero-extended), unlike i32.
        diff_typed_call(
            "fn (x : u32, y : u32) -> u32 ( x + y )",
            "f(1000000000, 2000000000)",
            3_000_000_000,
        );
    }

    #[test]
    fn signed_vs_unsigned_comparison_matches_between_tiers() {
        // The same byte 0xFF compares differently as i8 (-1) and u8 (255). 255 has
        // no exact i8, so the i8 side takes it through the explicit wrapping cast.
        diff_typed_call("fn (x : i8) -> i32 ( if (x < 1) (100) else (200) )", "f(i8(255))", 100);
        diff_typed_call("fn (x : u8) -> i32 ( if (x < 1) (100) else (200) )", "f(255)", 200);
    }

    /// Diff a nullary `fn () -> ... ( body )` between the interpreter and the JIT, where
    /// `body` reads an enclosing variable `a` of numeric type `nt` initialised to the
    /// low `nt.bytes()` of `init` (its bit-container). Floats can't ride the i32-mold
    /// argument path, so a float operand has to be a real stored variable, not a call
    /// argument; the body only reads `a`, so no reset between runs is needed.
    fn diff_var_fn(nt: NumType, init: i64, fn_src: &str, expect: i64) {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&init.to_ne_bytes()[..nt.bytes()]);
        let a = store.alloc_raw(core.numtypes[nt as usize], a_val);
        scopes.declare(&mut trie, "a", a).unwrap();
        let func = {
            let mut p = Parser::new(fn_src, &mut store, &mut trie, &core.metas, core.types(), scopes);
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
        let interp = unsafe { rt.run(call) }.unwrap();
        // SAFETY: `func`/`a` live in `store`, which outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        assert_eq!(interp, expect, "interpreter: {fn_src}");
        assert_eq!(jit, interp, "jit != interpreter: {fn_src}");
    }

    #[test]
    fn f64_arithmetic_matches_between_tiers() {
        // f64 add with a molded `1.5` beside a typed f64 variable (2.5): 2.5 + 1.5 = 4.0.
        // Result is the f64 bit-container, so both tiers must reinterpret bits the same
        // way across the ABI (interpreter read/mold vs JIT bitcast).
        diff_var_fn(NumType::F64, 2.5f64.to_bits() as i64, "fn () -> f64 ( a + 1.5 )", 4.0f64.to_bits() as i64);
        diff_var_fn(NumType::F64, 2.5f64.to_bits() as i64, "fn () -> f64 ( a - 0.5 )", 2.0f64.to_bits() as i64);
    }

    #[test]
    fn f32_arithmetic_matches_between_tiers() {
        // f32 add: the f32 bits ride the low 32 of the container (zero-extended), a
        // different ABI path than f64, so it is worth its own diff.
        diff_var_fn(NumType::F32, i64::from(2.5f32.to_bits()), "fn () -> f32 ( a + 1.5 )", i64::from(4.0f32.to_bits()));
    }

    #[test]
    fn f64_comparison_matches_between_tiers() {
        // f64 `<` (an fcmp), true and false, gated through `if` so the result is the
        // i32 bool the comparison yields. a = 2.5.
        diff_var_fn(NumType::F64, 2.5f64.to_bits() as i64, "fn () -> i32 ( if (a < 3.0) (100) else (200) )", 100);
        diff_var_fn(NumType::F64, 2.5f64.to_bits() as i64, "fn () -> i32 ( if (a < 2.0) (100) else (200) )", 200);
    }

    /// Parse `src` at expression scope and return the error it fails with.
    fn parse_err(src: &str) -> ParseError {
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(src, &mut store, &mut trie, &core.metas, core.types(), s);
        p.parse_expression().unwrap_err()
    }

    #[test]
    fn casts_between_integer_widths_match_between_tiers() {
        // Widen sign-extends (i8 -1 kept as i32 -1); narrow drops high bits
        // (i32 300 -> i8 44); a same-width reinterpret keeps the bits (u32 3e9 -> i32
        // is the negative reading); a same-type cast is the operand unchanged. 255
        // has no exact i8, so reaching an i8 parameter takes an explicit `i8(255)`
        // (the wrapping cast), never a bare literal.
        diff_typed_call("fn (x : i8) -> i32 ( i32(x) )", "f(i8(255))", -1);
        diff_typed_call("fn (x : i32) -> i8 ( i8(x) )", "f(300)", 44);
        diff_typed_call("fn (x : i8) -> u8 ( u8(x) )", "f(i8(255))", 255);
        diff_typed_call("fn (x : i32) -> i32 ( i32(x) )", "f(42)", 42);
        // u32 3e9 reinterpreted as i32: 3_000_000_000 - 2^32.
        diff_var_fn(NumType::U32, 3_000_000_000, "fn () -> i32 ( i32(a) )", 3_000_000_000i64 - 4_294_967_296);
        // u64 300 -> u8 wraps to 44.
        diff_var_fn(NumType::U64, 300, "fn () -> u8 ( u8(a) )", 44);
    }

    #[test]
    fn casts_between_int_and_float_match_between_tiers() {
        // int -> float is exact for small values; float -> int truncates toward zero and
        // *saturates* out of range (matching Rust `as`, so both tiers agree).
        diff_typed_call("fn (x : i32) -> f64 ( f64(x) )", "f(3)", 3.0f64.to_bits() as i64);
        diff_var_fn(NumType::F64, 3.7f64.to_bits() as i64, "fn () -> i32 ( i32(a) )", 3);
        diff_var_fn(NumType::F64, (-3.7f64).to_bits() as i64, "fn () -> i32 ( i32(a) )", -3);
        diff_var_fn(NumType::F64, 1e20f64.to_bits() as i64, "fn () -> i32 ( i32(a) )", i64::from(i32::MAX));
        diff_var_fn(NumType::F64, (-1e20f64).to_bits() as i64, "fn () -> i32 ( i32(a) )", i64::from(i32::MIN));
        // f64 -> f32 demote.
        diff_var_fn(NumType::F64, 1.5f64.to_bits() as i64, "fn () -> f32 ( f32(a) )", i64::from(1.5f32.to_bits()));
    }

    #[test]
    fn casts_fold_literal_operands() {
        // A literal operand converts at parse time with `as` semantics: an integer stays
        // exact, a decimal truncates toward zero, a float target takes the value.
        diff_typed_call("fn () -> i32 ( i32(3) )", "f()", 3);
        diff_typed_call("fn () -> i32 ( i32(3.5) )", "f()", 3);
        diff_typed_call("fn () -> u8 ( u8(300) )", "f()", 44);
        diff_typed_call("fn () -> f64 ( f64(2) )", "f()", 2.0f64.to_bits() as i64);
    }

    #[test]
    fn malformed_casts_are_rejected() {
        assert_eq!(parse_err("i32()"), ParseError::BadCast); // no operand
        assert_eq!(parse_err("i32(1, 2)"), ParseError::BadCast); // too many operands
        assert_eq!(parse_err("i32(struct ())"), ParseError::BadCast); // non-numeric operand
    }

    #[test]
    fn void_function_yields_unit_both_tiers() {
        // A `-> void` fn discards its body value and returns unit (0 bits) in both tiers.
        diff_typed_call("fn () -> void ( 42 )", "f()", 0);
    }

    #[test]
    fn void_function_runs_its_body_for_effect() {
        // The void body still executes: `a = a + 1` bumps the enclosing variable, and
        // the fn returns unit. Diffed between tiers on both the return and the effect.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);
        let a_val = store.alloc_bytes(&41i32.to_ne_bytes());
        let a = store.alloc_raw(core.i32_, a_val);
        scopes.declare(&mut trie, "a", a).unwrap();
        let func = {
            let mut p = Parser::new(
                "fn () -> void ( a = a + 1 )",
                &mut store,
                &mut trie,
                &core.metas,
                core.types(),
                scopes,
            );
            p.parse_expression().unwrap()
        };
        let call = store.alloc_raw(func, std::ptr::null_mut());
        let mut rt = Runtime::new(core.fn_type, core.rational, core.struct_, &core.bcode);
        // Interpreted: yields unit 0, leaves a = 42.
        // SAFETY: `call`/`func`/`a` are valid nodes just built in `store`.
        let interp = unsafe { rt.run(call) }.unwrap();
        let interp_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };
        // Reset a, compile (installs bcode), run again — jumps to the compiled body.
        unsafe { std::ptr::write_unaligned(a_val as *mut i32, 41) };
        // SAFETY: `func`/`a` live in `store`, which outlives the call.
        let _c = unsafe { compile_fn(&core.lower, core.types(), func) }.unwrap();
        let jit = unsafe { rt.run(call) }.unwrap();
        let jit_a = unsafe { std::ptr::read_unaligned(a_val as *const i32) };
        assert_eq!(interp, 0, "void yields unit (interpreted)");
        assert_eq!(jit, 0, "void yields unit (compiled)");
        assert_eq!(interp_a, 42, "body ran (interpreted)");
        assert_eq!(jit_a, 42, "body ran (compiled)");
    }

    #[test]
    fn both_literal_arithmetic_stays_rational() {
        // `1 + 2` is not committed to i32 at parse time; it folds to a rational literal
        // (exact), committing only when context types it.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let node = {
            let mut p = Parser::new("1 + 2", &mut store, &mut trie, &core.metas, core.types(), s);
            p.parse_expression().unwrap()
        };
        // SAFETY: `node` is the folded literal just parsed.
        unsafe { assert_eq!((*node).ty, core.rational, "1 + 2 stays a rational literal") };
    }

    #[test]
    fn comptime_rational_arithmetic_folds_exactly_and_commits_on_context() {
        // Two comptime literals stay rational and fold exactly: `1000000 * 1000000` is
        // 10^12 (not an i32 overflow), committing to i64 through the cast; `2e9 + 2e9`
        // commits to the i64 return type; a decimal fold reduces the fraction exactly.
        diff_typed_call("fn () -> i64 ( i64(1000000 * 1000000) )", "f()", 1_000_000_000_000);
        diff_typed_call("fn () -> i64 ( 2000000000 + 2000000000 )", "f()", 4_000_000_000);
        diff_typed_call("fn () -> i64 ( 2000000000 * 2 )", "f()", 4_000_000_000);
        // The commit reaches through an explicit `return` too (not just a bare tail).
        diff_typed_call("fn () -> i64 ( return 2000000000 + 2000000000 )", "f()", 4_000_000_000);
        // 0.5 + 0.25 = 3/4 exactly, then to f64 bits.
        diff_typed_call("fn () -> f64 ( f64(0.5 + 0.25) )", "f()", 0.75f64.to_bits() as i64);
    }

    #[test]
    fn comptime_rational_comparison_folds() {
        // Comparing two comptime literals folds to a `bool`, so it works even for values
        // with no i32 — an i32 compare could not commit `3000000000`.
        diff_typed_call("fn () -> i32 ( if (3000000000 < 4000000000) (1) else (0) )", "f()", 1);
        diff_typed_call("fn () -> i32 ( if (5 > 3) (1) else (0) )", "f()", 1);
    }

    #[test]
    fn a_comptime_rational_that_overflows_i64_is_rejected() {
        // The seed's rationals are i64 fractions; an exact product past i64 has no
        // representation, a clean error rather than a wrong wrapped value.
        assert_eq!(parse_err("i64(10000000000 * 10000000000)"), ParseError::UncomputableLiteral);
    }

    #[test]
    fn comptime_rational_commits_through_if_branches() {
        // An `if` in tail position is a value slot too: a large comptime rational in
        // either branch commits to the i64 return type (it would otherwise fail the i32
        // mold shim). This also exercises the width-general `if` lowering (i64 branches).
        let fn_src = "fn (c : i32) -> i64 ( if (c < 1) (2000000000 + 2000000000) else (0) )";
        diff_typed_call(fn_src, "f(0)", 4_000_000_000); // then-branch taken
        diff_typed_call(fn_src, "f(5)", 0); // else-branch taken
        // The else-branch commits too.
        let fn_src = "fn (c : i32) -> i64 ( if (c < 1) (0) else (2000000000 + 2000000000) )";
        diff_typed_call(fn_src, "f(5)", 4_000_000_000);
    }

    #[test]
    fn different_concrete_types_are_a_mismatch() {
        // Cross-type arithmetic needs an explicit cast; there is no implicit coercion.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x : i32, y : i64) -> i64 ( x + y )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            s,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::TypeMismatch));
    }

    #[test]
    fn a_literal_that_does_not_fit_its_committed_type_is_rejected() {
        // `1.5` beside an i32 has no exact i32, so committing it fails at parse time.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut s = ScopeStack::new();
        s.push(core.root_scope);
        let mut p = Parser::new(
            "fn (x : i32) -> i32 ( x + 1.5 )",
            &mut store,
            &mut trie,
            &core.metas,
            core.types(),
            s,
        );
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UncomputableLiteral));
    }

    #[test]
    fn plus_over_non_numeric_operands_is_unresolved() {
        // `+` with a non-numeric operand (a struct value) has no concrete machine op
        // to resolve to, so parsing reports UnsupportedOperands.
        let mut store = Store::new();
        let mut trie = RegexTrie::new();
        let core = Core::build(&mut store, &mut trie);
        let mut scopes = ScopeStack::new();
        scopes.push(core.root_scope);

        let mut p =
            Parser::new("struct () + 1", &mut store, &mut trie, &core.metas, core.types(), scopes);
        assert_eq!(p.parse_expression(), Err(crate::parse::ParseError::UnsupportedOperands));
    }
}
