// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The eleven infix operators over two numeric operands — `+ - * / %` and
//! `< > <= >= == !=`. Each is a parse-time constructor owning no code
//! (DESIGN ›The callable ground is `@exec`‹; issue #44): from its operand
//! types it resolves each application to one concrete machine operation and
//! stores that leaf in the node's op slot — `{type: +, value: [lhs, rhs,
//! add_i32]}` — so run jumps through the node and compile reads the same
//! resolution. One identity serves every numeric type; the concrete
//! operations are the callable leaves of [`super::ops`].
//!
//! What differs per operator is data — its spelling, its place on the one
//! parse_rank axis, its family and machine operation — held in one table
//! here ([`register_all`]) as [`super::ops`] holds the leaves, and the
//! resolution rule is written once ([`build_op`]): two comptime rationals
//! fold now (exact fraction math, a comparison to a `bool` literal), and
//! otherwise [`resolve_binary`] settles the operand type — matching concrete
//! types keep theirs, a literal molds to its partner — and the leaf for that
//! type goes in the op slot. Three operators carry one extra each: `-`
//! opening with no left operand negates the operand to its right; `%` has no
//! float leaf; `==` and `!=` compare two identities at parse and two
//! addresses at run before the numeric rule.

use cranelift_codegen::ir::Value;

use super::numtype::{ArithOp, CmpOp, NumType};
use super::read::Read;
use super::{bool_mod, meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, LowerFn, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ConstructFn, ParseError};
use crate::store::Store;
use crate::Core;

/// The machine family an operator resolves into, with its operation.
#[derive(Clone, Copy)]
pub(super) enum Family {
    Arith(ArithOp),
    Cmp(CmpOp),
}

/// The eleven identities, as [`register_all`] mints them.
pub(super) struct BinaryIds {
    pub plus: DyadPtr,
    pub minus: DyadPtr,
    pub times: DyadPtr,
    pub div: DyadPtr,
    pub rem: DyadPtr,
    pub lt: DyadPtr,
    pub gt: DyadPtr,
    pub eq: DyadPtr,
    pub le: DyadPtr,
    pub ge: DyadPtr,
    pub ne: DyadPtr,
}

/// Register the eleven operators: spelling, parse_rank (all left-associative:
/// `+`/`-` additive, `*`/`/`/`%` multiplicative, the relational four above
/// the two equalities, the trie longest-matching `<=` over `<` and `==`
/// over `=`), constructor and lowering. The order is the index's insertion
/// order.
pub(super) fn register_all(cx: &mut Cx) -> BinaryIds {
    use meta::prec::{ADDITIVE, COMPARE, EQUALITY, MULTIPLICATIVE};
    BinaryIds {
        plus: register(cx, "+", ADDITIVE, Family::Arith(ArithOp::Add)),
        minus: register(cx, "-", ADDITIVE, Family::Arith(ArithOp::Sub)),
        times: register(cx, "*", MULTIPLICATIVE, Family::Arith(ArithOp::Mul)),
        div: register(cx, "/", MULTIPLICATIVE, Family::Arith(ArithOp::Div)),
        rem: register(cx, "%", MULTIPLICATIVE, Family::Arith(ArithOp::Rem)),
        lt: register(cx, "<", COMPARE, Family::Cmp(CmpOp::Lt)),
        gt: register(cx, ">", COMPARE, Family::Cmp(CmpOp::Gt)),
        eq: register(cx, "==", EQUALITY, Family::Cmp(CmpOp::Eq)),
        le: register(cx, "<=", COMPARE, Family::Cmp(CmpOp::Le)),
        ge: register(cx, ">=", COMPARE, Family::Cmp(CmpOp::Ge)),
        ne: register(cx, "!=", EQUALITY, Family::Cmp(CmpOp::Ne)),
    }
}

/// Register one operator: a plain type whose record is parse and layout
/// metadata (`[lhs, rhs, op]`); the executable code lives on the leaves its
/// applications reference.
fn register(cx: &mut Cx, spelling: &str, rank: f64, family: Family) -> DyadPtr {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, rank, Assoc::Left, &["lhs", "rhs", "op"]);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(spelling, id);
    let (construct, lower) = shims(family);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

const ARITH: u8 = 0;
const CMP: u8 = 1;
const ADD: u8 = ArithOp::Add as u8;
const SUB: u8 = ArithOp::Sub as u8;
const MUL: u8 = ArithOp::Mul as u8;
const DIV: u8 = ArithOp::Div as u8;
const REM: u8 = ArithOp::Rem as u8;
const LT: u8 = CmpOp::Lt as u8;
const GT: u8 = CmpOp::Gt as u8;
const LE: u8 = CmpOp::Le as u8;
const GE: u8 = CmpOp::Ge as u8;
const EQ: u8 = CmpOp::Eq as u8;
const NE: u8 = CmpOp::Ne as u8;

/// The monomorphic constructor and lowering of an operator, its family and
/// operation baked in as const generics (as [`super::ops`] bakes its
/// leaves): a table entry is a plain fn pointer, so each pair is its own
/// function. `-` has its own constructor, for its prefix form.
fn shims(family: Family) -> (ConstructFn, LowerFn) {
    match family {
        Family::Arith(ArithOp::Add) => {
            (super::infix_construct!(build::<ARITH, ADD>), lower::<ARITH, ADD>)
        }
        Family::Arith(ArithOp::Sub) => (construct_minus, lower::<ARITH, SUB>),
        Family::Arith(ArithOp::Mul) => {
            (super::infix_construct!(build::<ARITH, MUL>), lower::<ARITH, MUL>)
        }
        Family::Arith(ArithOp::Div) => {
            (super::infix_construct!(build::<ARITH, DIV>), lower::<ARITH, DIV>)
        }
        Family::Arith(ArithOp::Rem) => {
            (super::infix_construct!(build::<ARITH, REM>), lower::<ARITH, REM>)
        }
        Family::Cmp(CmpOp::Lt) => (super::infix_construct!(build::<CMP, LT>), lower::<CMP, LT>),
        Family::Cmp(CmpOp::Gt) => (super::infix_construct!(build::<CMP, GT>), lower::<CMP, GT>),
        Family::Cmp(CmpOp::Le) => (super::infix_construct!(build::<CMP, LE>), lower::<CMP, LE>),
        Family::Cmp(CmpOp::Ge) => (super::infix_construct!(build::<CMP, GE>), lower::<CMP, GE>),
        Family::Cmp(CmpOp::Eq) => (super::infix_construct!(build::<CMP, EQ>), lower::<CMP, EQ>),
        Family::Cmp(CmpOp::Ne) => (super::infix_construct!(build::<CMP, NE>), lower::<CMP, NE>),
    }
}

/// The family and operation a shim's const generics name.
fn family_of<const FAMILY: u8, const OP: u8>() -> Family {
    if FAMILY == ARITH {
        Family::Arith(ArithOp::from_tag(OP))
    } else {
        Family::Cmp(CmpOp::from_tag(OP))
    }
}

/// [`build_op`] with the operator baked in, the shape [`super::infix_construct`] takes.
fn build<const FAMILY: u8, const OP: u8>(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    build_op(store, types, op, family_of::<FAMILY, OP>(), lhs, rhs)
}

/// Build `lhs <op> rhs`: fold two comptime rationals now (exact fraction
/// math, a comparison to a `bool` literal), else resolve the operand type
/// ([`resolve_binary`]: matching concrete types keep theirs, a literal
/// molds to its partner, two literals fold exactly; non-numeric operands are
/// [`ParseError::UnsupportedOperands`]) and store the leaf for that type in
/// the op slot: `{type: op, value: [lhs, rhs, leaf]}`.
fn build_op(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    family: Family,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let slots = match family {
        Family::Arith(a) => {
            if let Some(folded) = rational::fold_arith(store, types, a, lhs, rhs)? {
                return Ok(folded);
            }
            let leaf = types.ops.rational_arith_leaf(a);
            // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
            if let Some(slots) = unsafe { rational_slots(store, types, lhs, rhs, leaf) }? {
                slots
            } else {
                // SAFETY: as above.
                let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
                // `%` over floats mints no leaf: there is no machine float
                // remainder (Cranelift has none), so the node cannot exist.
                if matches!(a, ArithOp::Rem) && nt.is_float() {
                    return Err(ParseError::UnsupportedOperands);
                }
                [lhs, rhs, types.ops.arith_leaf(a, nt)]
            }
        }
        Family::Cmp(c) => {
            if let Some(v) = rational::compare_literals(types, c, lhs, rhs) {
                return Ok(bool_mod::literal_node(store, types.bool_, v));
            }
            let leaf = types.ops.rational_cmp_leaf(c);
            // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
            if let Some(slots) = unsafe { rational_slots(store, types, lhs, rhs, leaf) }? {
                let value = store.alloc_operands(&slots);
                return Ok(store.alloc_raw(op, value));
            }
            if matches!(c, CmpOp::Eq | CmpOp::Ne) {
                if let Some(node) = build_identity_compare(store, types, op, c, lhs, rhs) {
                    return Ok(node);
                }
            }
            // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
            let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
            [lhs, rhs, types.ops.cmp_leaf(c, nt)]
        }
    };
    let value = store.alloc_operands(&slots);
    Ok(store.alloc_raw(op, value))
}

/// A rational operation at run time (#133 slice 8, part 4; DESIGN ›Numeric
/// literals are uncommitted until context classifies them‹: the result
/// "*stays* `rational_number`"): when either side is a rational value, the
/// other must be one too, or a literal, which is boxed
/// ([`rational::rational_operand`]); a concrete number beside a rational is
/// the mismatch, crossing being explicit. `None` when neither side is one.
///
/// # Safety
/// `lhs`/`rhs` are valid dyads from the store.
unsafe fn rational_slots(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
    rhs: DyadPtr,
    leaf: DyadPtr,
) -> Result<Option<[DyadPtr; 3]>, ParseError> {
    if !rational::is_rational_value(types, lhs) && !rational::is_rational_value(types, rhs) {
        return Ok(None);
    }
    let l = rational::rational_operand(store, types, lhs).ok_or(ParseError::TypeMismatch)?;
    let r = rational::rational_operand(store, types, rhs).ok_or(ParseError::TypeMismatch)?;
    Ok(Some([l, r, leaf]))
}

/// `==`/`!=` over what is not two numbers. What the two operands are is the
/// reading rule's answer (#82). Two identities in hand compare by identity
/// now: types are interned, so pointer identity *is* type identity (roadmap
/// #30) — `x:dyad.type == i32` is decided at parse. Two values that read as
/// node addresses — an identity against a box or a view, two boxes, a bare
/// parameter that "accepts any dyad" — compare those addresses when the
/// program runs (DESIGN ›A type is a comptime value‹, 12 September 2026: a
/// type value "may be passed to a function, held in a place, and compared"),
/// and so do two pointers (›Declarations are immutable by default‹: "`&x`
/// and `&y` differ"; #123). `None` hands the numeric rule the rest.
fn build_identity_compare(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    c: CmpOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Option<DyadPtr> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (l, r) =
        unsafe { (super::read::read_kind(types, lhs), super::read::read_kind(types, rhs)) };
    if l == Read::Identity && r == Read::Identity {
        // SAFETY: as above.
        let same = unsafe { types.through(lhs) == types.through(rhs) };
        let truth = if matches!(c, CmpOp::Eq) { same } else { !same };
        return Some(bool_mod::literal_node(store, types.bool_, truth));
    }
    let addressed = |k: Read| matches!(k, Read::Identity | Read::Container(_) | Read::Address);
    // SAFETY: as above.
    let pointer =
        |n: DyadPtr| unsafe { matches!(super::numtype_of(types, n), super::Operand::Pointer(_)) };
    if (addressed(l) && addressed(r)) || (pointer(lhs) && pointer(rhs)) {
        let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(c, NumType::I64)]);
        return Some(store.alloc_raw(op, value));
    }
    None
}

/// `-`'s constructor. At reduction (two completed operands flanking the
/// cursor) it is ordinary subtraction. Opening fresh — no left operand — it
/// negates the operand to its right (DESIGN ›Numeric literals‹, ruled 5
/// September 2026: one identity whose constructor reads its left context),
/// spelled `0 - x`, the sketch's own spelling of a negative, so it molds to
/// the operand's type and lowers as a subtraction. A literal to the right was
/// already folded into a negative literal at discovery by the literal's own
/// constructor, so this is the non-literal case.
fn construct_minus(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    if let Some((lhs, rhs)) = p.binary_operands(tape)? {
        let types = p.types();
        let node = build::<ARITH, SUB>(p.store(), types, id, lhs, rhs)?;
        tape.reduce_here(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let types = p.types();
    let rhs = p.take_right(tape)?;
    let zero = rational::build(p.store(), types.rational, "0")?;
    let node = build::<ARITH, SUB>(p.store(), types, id, zero, rhs)?;
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// Lower: emit the machine operation for the resolved operand type.
fn lower<const FAMILY: u8, const OP: u8>(
    lw: &mut Lowerer,
    node: DyadPtr,
) -> Result<Value, CompileError> {
    // SAFETY: `node` is a resolved application of this operator, `[lhs, rhs, op]`.
    unsafe {
        match family_of::<FAMILY, OP>() {
            Family::Arith(a) => lw.lower_arith(node, a),
            Family::Cmp(c) => lw.lower_compare(node, c),
        }
    }
}
