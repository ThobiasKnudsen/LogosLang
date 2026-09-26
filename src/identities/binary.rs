// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The eleven infix operators over two numeric operands, `+ - * / %` and
//! `< > <= >= == !=`. Each resolves an application to one concrete leaf from its
//! operand types and stores it in the node's op slot, `[lhs, rhs, leaf]`; two
//! comptime rationals fold at parse instead.

use cranelift_codegen::ir::Value;

use super::numtype::{ArithOp, CmpOp, NumType};
use super::read::Read;
use super::{bool_mod, meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, LowerFn, Lowerer};
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

/// A table entry is a plain fn pointer, so each (family, op) pair is its own function;
/// `-` has its own constructor for its prefix form.
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
    build_op(store, types, op, family_of::<FAMILY, OP>(), lhs, rhs)
}

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
            // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
            if let Some(node) = unsafe { pointer_step(store, types, op, a, lhs, rhs) }? {
                return Ok(node);
            }
            let leaf = types.ops.rational_arith_leaf(a);
            // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
            if let Some(slots) = unsafe { rational_slots(store, types, lhs, rhs, leaf) }? {
                slots
            } else {
                // SAFETY: as above.
                let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
                // No machine float remainder exists, so the node cannot.
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

/// `p + k` and `p - k` on an `@T` step k whole cells: the node is `[p, k * width, i64 op]`,
/// the scaling written into the graph as an `i64` product. `None` when `lhs` is no pointer.
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
    let super::Operand::Pointer(pointee) = super::numtype_of(types, lhs) else {
        return Ok(None);
    };
    if !matches!(a, ArithOp::Add | ArithOp::Sub) {
        return Err(ParseError::UnsupportedOperands);
    }
    let (_, width) =
        super::read::place_layout(types, pointee).ok_or(ParseError::UnsupportedOperands)?;
    let i64_ty = types.numtypes[NumType::I64 as usize];
    let offset = match super::numtype_of(types, rhs) {
        super::Operand::Literal => {
            let k = rational::mold_to(types.through(rhs), NumType::I64)
                .ok_or(ParseError::UncomputableLiteral)?;
            let bytes = k.checked_mul(width as i64).ok_or(ParseError::UncomputableLiteral)?;
            let value = store.alloc_bytes(&bytes.to_ne_bytes());
            store.alloc_raw(i64_ty, value)
        }
        super::Operand::Concrete(nt) if !nt.is_float() => {
            let k = super::build_cast(store, types, i64_ty, &[rhs])?;
            let value = store.alloc_bytes(&(width as i64).to_ne_bytes());
            let w = store.alloc_raw(i64_ty, value);
            let value =
                store.alloc_operands(&[k, w, types.ops.arith_leaf(ArithOp::Mul, NumType::I64)]);
            store.alloc_raw(types.times, value)
        }
        _ => return Err(ParseError::UnsupportedOperands),
    };
    let value = store.alloc_operands(&[lhs, offset, types.ops.arith_leaf(a, NumType::I64)]);
    Ok(Some(store.alloc_raw(op, value)))
}

/// When either side is a rational value, the other must be one too or a literal (boxed);
/// a concrete number beside a rational is the mismatch. `None` when neither side is one.
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
    // A tape cell compares as the identity it names, read at run as its `:type` is.
    // SAFETY: as above.
    let mut cell = |n: DyadPtr| unsafe {
        if (*types.through(n)).ty == types.tape.slot {
            super::tape::build_cell_identity(store, types, types.through(n))
        } else {
            n
        }
    };
    let (lhs, rhs) = (cell(lhs), cell(rhs));
    let addressed = |n: DyadPtr, k: Read| {
        matches!(k, Read::Identity | Read::Container(_) | Read::Address)
            // SAFETY: as above.
            // A type whose body is still open, named in its own parse, has no record yet.
            || unsafe {
                let ty = (*types.through(n)).ty;
                ty == types.tape.cell_type || ty == types.tape.cell_identity || ty == types.type_
            }
            // SAFETY: as above.
            || unsafe { super::yields_type(types, n) }
    };
    // SAFETY: as above.
    let pointer =
        |n: DyadPtr| unsafe { matches!(super::numtype_of(types, n), super::Operand::Pointer(_)) };
    if (addressed(lhs, l) && addressed(rhs, r)) || (pointer(lhs) && pointer(rhs)) {
        let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(c, NumType::I64)]);
        return Some(store.alloc_raw(op, value));
    }
    None
}

/// At reduction it is subtraction. Opening fresh, with no left operand, it negates the
/// operand to its right as `0 - x`, so it molds to the operand's type and lowers as a
/// subtraction; a literal to the right was already folded at discovery.
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
