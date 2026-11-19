// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The eleven infix operators over two operands, `+ - * / %` and `< > <= >= == !=`, and
//! negation, which `-` spells too and which builds `0 - x`. Their fields are `?`, so each
//! node is `[mint, lhs, rhs]`: the parse chooses the mint from the operand types once they
//! meet, and the mint holds the output type and the leaf; two comptime rationals fold at
//! parse instead.

use cranelift_codegen::ir::Value;

use super::numtype::{ArithOp, CmpOp, NumType};
use super::read::Read;
use super::{bool_mod, group, meta, mint, rational, resolve_binary, Cx};
use crate::compile::{CompileError, LowerFn, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ConstructFn, ParseError};
use crate::store::Store;
use crate::Core;

#[derive(Clone, Copy)]
pub(super) enum Family {
    Arith(ArithOp),
    Cmp(CmpOp),
}

pub(super) struct BinaryIds {
    pub plus: DyadPtr,
    pub minus: DyadPtr,
    pub negate: DyadPtr,
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

/// All left-associative; the trie longest-matches `<=` over `<` and `==` over `=`.
pub(super) fn register_all(cx: &mut Cx) -> BinaryIds {
    use meta::prec::{ADDITIVE, COMPARE, EQUALITY, MULTIPLICATIVE};
    BinaryIds {
        plus: register(cx, "+", ADDITIVE, Family::Arith(ArithOp::Add)),
        minus: register(cx, "-", ADDITIVE, Family::Arith(ArithOp::Sub)),
        negate: register_negate(cx),
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

fn register(cx: &mut Cx, spelling: &str, rank: f64, family: Family) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, rank, Assoc::Left, &["lhs", "rhs"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(spelling, id);
    let (construct, lower) = shims(family);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

/// Negation, a second identity of the spelling `-` beside subtraction, the lexer picking one by
/// what stands left of it. It takes the value to its right, so it ranks and associates as a
/// number type does (›`-` negates where a space stands before it and none after, or no value
/// stands to its left; any other `-` subtracts‹ [[R157]]).
fn register_negate(cx: &mut Cx) -> DyadPtr {
    let record = meta::record_assoc(cx.store, meta::TOKEN_TAG, meta::prec::CONVERT, Assoc::Right);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("-", id);
    cx.metas.insert(id, construct_negate);
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

/// A table entry is a plain fn pointer, so each (family, op) pair is its own function.
fn shims(family: Family) -> (ConstructFn, LowerFn) {
    match family {
        Family::Arith(ArithOp::Add) => {
            (super::infix_construct!(build::<ARITH, ADD>), lower::<ARITH, ADD>)
        }
        Family::Arith(ArithOp::Sub) => {
            (super::infix_construct!(build::<ARITH, SUB>), lower::<ARITH, SUB>)
        }
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

fn family_of<const FAMILY: u8, const OP: u8>() -> Family {
    if FAMILY == ARITH {
        Family::Arith(ArithOp::from_tag(OP))
    } else {
        Family::Cmp(CmpOp::from_tag(OP))
    }
}

fn build<const FAMILY: u8, const OP: u8>(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: a constructor hands its builder reduced dyads from the store.
    unsafe { build_op(store, types, op, family_of::<FAMILY, OP>(), lhs, rhs) }
}

/// # Safety
/// `lhs`/`rhs` must be reduced dyads from the store.
unsafe fn build_op(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    family: Family,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    match family {
        Family::Arith(a) => {
            if let Some(folded) = rational::fold_arith(store, types, a, lhs, rhs)? {
                return Ok(folded);
            }
            if let Some(node) = pointer_step(store, types, op, a, lhs, rhs)? {
                return Ok(node);
            }
            if rational_operands(types, lhs, rhs)? {
                let leaf = types.ops.rational_arith_leaf(a);
                return Ok(node(store, types, op, lhs, rhs, types.rational, leaf));
            }
            let ([lhs, rhs], nt) = resolve_binary(store, types, lhs, rhs)?;
            // No machine float remainder exists, so the node cannot.
            if matches!(a, ArithOp::Rem) && nt.is_float() {
                return Err(ParseError::UnsupportedOperands);
            }
            let (output, leaf) = (types.numtypes[nt as usize], types.ops.arith_leaf(a, nt));
            Ok(node(store, types, op, lhs, rhs, output, leaf))
        }
        Family::Cmp(c) => {
            if matches!(c, CmpOp::Eq | CmpOp::Ne) {
                let equal = matches!(c, CmpOp::Eq);
                if let Some(node) =
                    super::hashmap::build_unknown_compare(store, types, equal, lhs, rhs)
                {
                    return Ok(node);
                }
            }
            if let Some(v) = rational::compare_literals(types, c, lhs, rhs) {
                return Ok(bool_mod::literal_node(store, types.bool_, v));
            }
            if rational_operands(types, lhs, rhs)? {
                let leaf = types.ops.rational_cmp_leaf(c);
                return Ok(node(store, types, op, lhs, rhs, types.bool_, leaf));
            }
            if matches!(c, CmpOp::Eq | CmpOp::Ne) {
                if let Some(node) = build_identity_compare(store, types, op, c, lhs, rhs) {
                    return Ok(node);
                }
                // Two values of one type with a machine form, a `bool` among them, are equal
                // when their bits are.
                let t = super::read::output_type(types, lhs);
                let same = (t == super::read::output_type(types, rhs))
                    .then(|| super::read::place_layout(types, t))
                    .flatten();
                if let Some((Read::Scalar(nt), _)) = same {
                    let leaf = types.ops.cmp_leaf(c, nt);
                    return Ok(node(store, types, op, lhs, rhs, types.bool_, leaf));
                }
            }
            let ([lhs, rhs], nt) = resolve_binary(store, types, lhs, rhs)?;
            let leaf = types.ops.cmp_leaf(c, nt);
            Ok(node(store, types, op, lhs, rhs, types.bool_, leaf))
        }
    }
}

/// `[mint, lhs, rhs]`, the mint `op`'s for the types its operands give.
///
/// # Safety
/// `lhs`/`rhs` must be reduced dyads from the store; `output_type` and `leaf` what every node of
/// `op` over these operand types gives back and runs.
unsafe fn node(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
    output_type: DyadPtr,
    leaf: DyadPtr,
) -> DyadPtr {
    let key = [super::read::output_type(types, lhs), super::read::output_type(types, rhs)];
    let mint = mint::of(store, types, op, &key, output_type, leaf);
    store.alloc_words(mint, &[lhs, rhs])
}

/// `p + k` and `p - k` on an `@T` step k whole cells: the node is `[p, k * width]` under the
/// operator's mint for `@T` and `i64`, the scaling written into the graph as an `i64` product.
/// `None` when `lhs` is no pointer.
/// DESIGN ›A pointer steps by whole cells‹.
///
/// # Safety
/// `lhs`/`rhs` are reduced dyads from the store.
unsafe fn pointer_step(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    a: ArithOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    let super::Operand::Pointer(pointee) = super::operand_of(types, lhs) else {
        return Ok(None);
    };
    if !matches!(a, ArithOp::Add | ArithOp::Sub) {
        return Err(ParseError::UnsupportedOperands);
    }
    let (_, width) =
        super::read::place_layout(types, pointee).ok_or(ParseError::UnsupportedOperands)?;
    let i64_ty = types.numtypes[NumType::I64 as usize];
    let offset = match super::operand_of(types, rhs) {
        super::Operand::Literal => {
            let k = rational::mold_to(types.through(rhs), NumType::I64)
                .ok_or(ParseError::UncomputableLiteral)?;
            let bytes = k.checked_mul(width as i64).ok_or(ParseError::LiteralArithmetic)?;
            store.alloc_blob(i64_ty, &bytes.to_ne_bytes())
        }
        super::Operand::Concrete(nt) if !nt.is_float() => {
            // The step's own reading: an index of any integer type counts cells as an `i64`.
            let k = if nt == NumType::I64 {
                rhs
            } else {
                super::convert::build_convert(
                    store,
                    types,
                    rhs,
                    types.numtypes[nt as usize],
                    i64_ty,
                )
            };
            let w = store.alloc_blob(i64_ty, &(width as i64).to_ne_bytes());
            let mul = types.ops.arith_leaf(ArithOp::Mul, NumType::I64);
            node(store, types, types.times, k, w, i64_ty, mul)
        }
        _ => return Err(ParseError::UnsupportedOperands),
    };
    let output = super::pointer::make_pointer_type(store, types.type_, pointee);
    let leaf = types.ops.arith_leaf(a, NumType::I64);
    Ok(Some(node(store, types, op, lhs, offset, output, leaf)))
}

/// The pointer the steps `p + k` and `p - k` start from, whose block holds every cell they
/// reach; `node` itself when it is no step.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn step_base(types: &Core, node: DyadPtr) -> DyadPtr {
    let node = types.through(node);
    let op = mint::origin(dyad::ty(node));
    if (op == types.plus || op == types.minus)
        && super::numtype::is_pointer_type(super::read::output_type(types, node))
    {
        return step_base(types, *(dyad::value(node) as *const DyadPtr));
    }
    node
}

/// When either side is a rational value, the other must be one too or a literal; a
/// concrete number beside a rational is the mismatch. `false` when neither side is one.
///
/// # Safety
/// `lhs`/`rhs` must be reduced dyads from the store.
unsafe fn rational_operands(types: &Core, lhs: DyadPtr, rhs: DyadPtr) -> Result<bool, ParseError> {
    if !rational::is_rational_value(types, lhs) && !rational::is_rational_value(types, rhs) {
        return Ok(false);
    }
    let fits = |n: DyadPtr| {
        rational::is_rational_value(types, n)
            || matches!(super::operand_of(types, n), super::Operand::Literal)
    };
    if !fits(lhs) || !fits(rhs) {
        return Err(ParseError::TypeMismatch);
    }
    Ok(true)
}

/// Two identities compare by identity at parse (types are interned); two values that
/// read as node addresses, or two pointers, compare those addresses at run. `None`
/// hands the numeric rule the rest.
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
    // A tape cell compares as the identity it names, read at run as its `.type` is.
    // SAFETY: as above.
    let mut cell = |n: DyadPtr| unsafe {
        if dyad::ty(types.through(n)) == types.tape.slot {
            super::tape::build_cell_identity(store, types, types.through(n))
        } else {
            n
        }
    };
    let (lhs, rhs) = (cell(lhs), cell(rhs));
    let addressed = |n: DyadPtr, k: Read| {
        matches!(k, Read::Identity | Read::Container(_) | Read::Address | Read::Node)
            // SAFETY: as above.
            || unsafe { super::read::output_type(types, n) == types.type_ }
    };
    // SAFETY: as above.
    let pointer =
        |n: DyadPtr| unsafe { matches!(super::operand_of(types, n), super::Operand::Pointer(_)) };
    if (addressed(lhs, l) && addressed(rhs, r)) || (pointer(lhs) && pointer(rhs)) {
        let leaf = types.ops.cmp_leaf(c, NumType::I64);
        // SAFETY: as above; a cell's identity read is built over a reduced dyad too.
        return Some(unsafe { node(store, types, op, lhs, rhs, types.bool_, leaf) });
    }
    None
}

/// The value to its right negated as `0 - x`, a subtraction's node, so it molds to the value's
/// type and lowers as a subtraction; a literal to the right folded it at discovery.
fn construct_negate(
    p: &mut crate::parse::Parser,
    _id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let types = p.types();
    let rhs = p.take_right(tape)?;
    let zero = rational::build(p.store(), types.rational, "0")?;
    let node = group::apply(p.store(), types, types.minus, zero, rhs, build::<ARITH, SUB>)?;
    // SAFETY: `node` was just built over reduced dyads.
    let node = unsafe { rational::slotted(p, node) };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

fn lower<const FAMILY: u8, const OP: u8>(
    lw: &mut Lowerer,
    node: DyadPtr,
) -> Result<Value, CompileError> {
    // SAFETY: `node` is a node of one of this operator's mints, `[lhs, rhs]`.
    unsafe {
        match family_of::<FAMILY, OP>() {
            Family::Arith(a) => lw.lower_arith(node, a),
            Family::Cmp(c) => lw.lower_compare(node, c),
        }
    }
}
