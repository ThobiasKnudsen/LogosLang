// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The infix operators over two operands, `+ - * / %`, `< > <= >= == !=` and `and or xor
//! implies`, and negation, which `-` spells too. Each node is `[mint, lhs, rhs]`, or negation's
//! `[mint, operand]`: the parse converts the operands until they meet, and each operator's chooser
//! makes its mint for their types, holding the output type and the leaf; comptime rationals and
//! two `bool` literals fold at parse instead.

use cranelift_codegen::ir::Value;

use super::numtype::{self, ArithOp, CmpOp, LogicOp, NumType};
use super::read::Read;
use super::{bool_mod, group, is_numtype_node, meta, mint, rational, resolve_binary, Cx};
use crate::compile::{CompileError, LowerFn, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{bool_literal_value, Assoc, ConstructFn, ParseError};
use crate::store::Store;
use crate::Core;

#[derive(Clone, Copy)]
pub(super) enum Family {
    Arith(ArithOp),
    Cmp(CmpOp),
    Logic(LogicOp),
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
    pub and_: DyadPtr,
    pub or_: DyadPtr,
}

/// All left-associative but `implies`; the trie longest-matches `<=` over `<` and `==` over `=`.
/// The logic words stand in four steps, `implies` last (›Every identity has a `parse_rank`, and
/// `(` works in two steps‹ [[R259]]).
pub(super) fn register_all(cx: &mut Cx) -> BinaryIds {
    use meta::prec;
    use Assoc::{Left, Right};
    register(cx, "xor", prec::OR, Left, Family::Logic(LogicOp::Xor));
    register(cx, "implies", prec::IMPLIES, Right, Family::Logic(LogicOp::Implies));
    BinaryIds {
        plus: register(cx, "+", prec::ADDITIVE, Left, Family::Arith(ArithOp::Add)),
        minus: register(cx, "-", prec::ADDITIVE, Left, Family::Arith(ArithOp::Sub)),
        negate: register_negate(cx),
        times: register(cx, "*", prec::MULTIPLICATIVE, Left, Family::Arith(ArithOp::Mul)),
        div: register(cx, "/", prec::MULTIPLICATIVE, Left, Family::Arith(ArithOp::Div)),
        rem: register(cx, "%", prec::MULTIPLICATIVE, Left, Family::Arith(ArithOp::Rem)),
        lt: register(cx, "<", prec::COMPARE, Left, Family::Cmp(CmpOp::Lt)),
        gt: register(cx, ">", prec::COMPARE, Left, Family::Cmp(CmpOp::Gt)),
        eq: register(cx, "==", prec::COMPARE, Left, Family::Cmp(CmpOp::Eq)),
        le: register(cx, "<=", prec::COMPARE, Left, Family::Cmp(CmpOp::Le)),
        ge: register(cx, ">=", prec::COMPARE, Left, Family::Cmp(CmpOp::Ge)),
        ne: register(cx, "!=", prec::COMPARE, Left, Family::Cmp(CmpOp::Ne)),
        and_: register(cx, "and", prec::AND, Left, Family::Logic(LogicOp::And)),
        or_: register(cx, "or", prec::OR, Left, Family::Logic(LogicOp::Or)),
    }
}

fn register(cx: &mut Cx, spelling: &str, rank: f64, assoc: Assoc, family: Family) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, rank, assoc, &["lhs", "rhs"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare(spelling, id);
    let (construct, lower, choose) = shims(family);
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    cx.choosers.insert(id, choose);
    id
}

/// Negation, a second identity of the spelling `-` beside subtraction, the lexer picking one by
/// what stands left of it, taking the value to its right (›`-` negates where a space stands
/// before it and none after, or no value stands to its left; any other `-` subtracts‹ [[R157]]).
fn register_negate(cx: &mut Cx) -> DyadPtr {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::NEGATE, Assoc::Right, &["operand"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("-", id);
    cx.metas.insert(id, construct_negate);
    cx.lower.insert(id, lower_negate);
    cx.choosers.insert(id, choose_negate);
    id
}

const ARITH: u8 = 0;
const CMP: u8 = 1;
const LOGIC: u8 = 2;
const AND: u8 = LogicOp::And as u8;
const OR: u8 = LogicOp::Or as u8;
const XOR: u8 = LogicOp::Xor as u8;
const IMPLIES: u8 = LogicOp::Implies as u8;
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
fn shims(family: Family) -> (ConstructFn, LowerFn, mint::ChooseFn) {
    macro_rules! entry {
        ($family:ident, $op:ident) => {
            (
                super::infix_construct!(build::<$family, $op>),
                lower::<$family, $op> as LowerFn,
                choose::<$family, $op> as mint::ChooseFn,
            )
        };
    }
    match family {
        Family::Arith(ArithOp::Add) => entry!(ARITH, ADD),
        Family::Arith(ArithOp::Sub) => entry!(ARITH, SUB),
        Family::Arith(ArithOp::Mul) => entry!(ARITH, MUL),
        Family::Arith(ArithOp::Div) => entry!(ARITH, DIV),
        Family::Arith(ArithOp::Rem) => entry!(ARITH, REM),
        Family::Cmp(CmpOp::Lt) => entry!(CMP, LT),
        Family::Cmp(CmpOp::Gt) => entry!(CMP, GT),
        Family::Cmp(CmpOp::Le) => entry!(CMP, LE),
        Family::Cmp(CmpOp::Ge) => entry!(CMP, GE),
        Family::Cmp(CmpOp::Eq) => entry!(CMP, EQ),
        Family::Cmp(CmpOp::Ne) => entry!(CMP, NE),
        Family::Logic(LogicOp::And) => entry!(LOGIC, AND),
        Family::Logic(LogicOp::Or) => entry!(LOGIC, OR),
        Family::Logic(LogicOp::Xor) => entry!(LOGIC, XOR),
        Family::Logic(LogicOp::Implies) => entry!(LOGIC, IMPLIES),
    }
}

fn family_of<const FAMILY: u8, const OP: u8>() -> Family {
    match FAMILY {
        ARITH => Family::Arith(ArithOp::from_tag(OP)),
        CMP => Family::Cmp(CmpOp::from_tag(OP)),
        _ => Family::Logic(LogicOp::from_tag(OP)),
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
                return node(store, types, op, lhs, rhs);
            }
            let ([lhs, rhs], _) = resolve_binary(store, types, lhs, rhs)?;
            node(store, types, op, lhs, rhs)
        }
        Family::Cmp(c) => {
            if matches!(c, CmpOp::Eq | CmpOp::Ne) {
                if let Some(node) =
                    super::hashmap::build_unknown_compare(store, types, op, lhs, rhs)?
                {
                    return Ok(node);
                }
            }
            if let Some(v) = rational::compare_literals(types, c, lhs, rhs) {
                return Ok(bool_mod::literal_node(store, types.bool_, v));
            }
            if rational_operands(types, lhs, rhs)? {
                return node(store, types, op, lhs, rhs);
            }
            if matches!(c, CmpOp::Eq | CmpOp::Ne) {
                if let Some(node) = build_identity_compare(store, types, op, c, lhs, rhs)? {
                    return Ok(node);
                }
                // Two values of one type with a machine form, a `bool` among them, are equal
                // when their bits are.
                let t = super::read::output_type(types, lhs);
                let same = (t == super::read::output_type(types, rhs))
                    .then(|| super::read::place_layout(types, t))
                    .flatten();
                if let Some((Read::Scalar(_), _)) = same {
                    return node(store, types, op, lhs, rhs);
                }
            }
            let ([lhs, rhs], _) = resolve_binary(store, types, lhs, rhs)?;
            node(store, types, op, lhs, rhs)
        }
        Family::Logic(l) => build_logic(store, types, op, l, lhs, rhs),
    }
}

/// Two `bool`s mint, and two literals fold; `and` and `or` over two non-booleans build a group,
/// the node `[lhs, rhs]` typed by the operation itself, which no chooser made (›`and`/`or` on
/// non-booleans build a group; every operator on a group applies to each member‹ [[R37]]).
///
/// # Safety
/// `lhs`/`rhs` must be reduced dyads from the store.
pub(super) unsafe fn build_logic(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    l: LogicOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let is_bool = |n: DyadPtr| super::read::output_type(types, n) == types.bool_;
    match (is_bool(lhs), is_bool(rhs)) {
        (true, true) => {}
        (false, false) if l.groups() => return Ok(store.alloc_words(op, &[lhs, rhs])),
        _ => return Err(ParseError::NonBoolOperands),
    }
    if let (Some(a), Some(b)) = (bool_literal_value(types, lhs), bool_literal_value(types, rhs)) {
        return Ok(bool_mod::literal_node(store, types.bool_, l.apply(a, b)));
    }
    node(store, types, op, lhs, rhs)
}

/// `[mint, lhs, rhs]`, the mint `op`'s chooser makes for the types its operands give.
///
/// # Safety
/// `lhs`/`rhs` must be reduced dyads from the store.
unsafe fn node(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let key = [super::read::output_type(types, lhs), super::read::output_type(types, rhs)];
    mint::node(store, types, op, &key, &[lhs, rhs])
}

/// stand-in for #462
///
/// # Safety
/// `key` must hold type nodes from the store, or null.
unsafe fn choose<const FAMILY: u8, const OP: u8>(
    store: &mut Store,
    types: &Core,
    key: &[DyadPtr],
) -> Option<mint::Made> {
    match family_of::<FAMILY, OP>() {
        Family::Arith(a) => choose_arith(store, types, a, key),
        Family::Cmp(c) => choose_compare(types, c, key),
        Family::Logic(l) => {
            let &[a, b] = key else {
                return None;
            };
            (a == types.bool_ && b == types.bool_)
                .then(|| mint::Made { output_type: types.bool_, op: types.ops.logic_leaf(l) })
        }
    }
}

/// What `a`'s mint over the field types `key` gives back and runs: a number type over two of it,
/// with no float remainder, since no machine one exists; a run-time rational over two; a pointer
/// stepped by an `i64` count of bytes, giving the plain pointer to the same type (›A pointer
/// steps by whole cells‹); stand-in for #462.
///
/// # Safety
/// `key` must hold type nodes from the store, or null.
unsafe fn choose_arith(
    store: &mut Store,
    types: &Core,
    a: ArithOp,
    key: &[DyadPtr],
) -> Option<mint::Made> {
    let &[l, r] = key else {
        return None;
    };
    let (output_type, op) = if numtype::is_pointer_type(l) {
        let i64_ = types.numtypes[NumType::I64 as usize];
        if !matches!(a, ArithOp::Add | ArithOp::Sub) || r != i64_ {
            return None;
        }
        let output = super::pointer::make_pointer_type(store, types, numtype::gated_pointee_of(l));
        (output, types.ops.arith_leaf(a, NumType::I64))
    } else if l == types.rational && r == types.rational {
        (types.rational, types.ops.rational_arith_leaf(a))
    } else {
        let nt = one_number_type(types, l, r)?;
        if matches!(a, ArithOp::Rem) && nt.is_float() {
            return None;
        }
        (types.numtypes[nt as usize], types.ops.arith_leaf(a, nt))
    };
    Some(mint::Made { output_type, op })
}

/// What `c`'s mint over the field types `key` runs, giving a `bool`: a comparison of two values
/// of one number type or of two run-time rationals; and for `==` and `!=`, of a map and `?`, of
/// two types by reading, of two values of one other machine type (a `bool`), of two values read
/// as the address of a node (a dyad, text) and of two pointers; stand-in for #462.
///
/// # Safety
/// `key` must hold type nodes from the store, or null.
unsafe fn choose_compare(types: &Core, c: CmpOp, key: &[DyadPtr]) -> Option<mint::Made> {
    let &[l, r] = key else {
        return None;
    };
    let equality = matches!(c, CmpOp::Eq | CmpOp::Ne);
    let addressed = |t: DyadPtr| matches!(super::read::place_kind(types, t), Read::Container(_));
    let unknown = if equality {
        super::hashmap::unknown_compare_leaf(types, matches!(c, CmpOp::Eq), l, r)
    } else {
        None
    };
    let op = if let Some(leaf) = unknown {
        leaf
    } else if l == types.rational && r == types.rational {
        types.ops.rational_cmp_leaf(c)
    } else if let Some(nt) = one_number_type(types, l, r) {
        types.ops.cmp_leaf(c, nt)
    } else if !equality {
        return None;
    } else if l == types.type_ && r == types.type_ {
        types.ops.type_cmp_leaf(matches!(c, CmpOp::Eq))
    } else if let (true, Some((Read::Scalar(nt), _))) =
        (l == r, super::read::place_layout(types, l))
    {
        types.ops.cmp_leaf(c, nt)
    } else if (addressed(l) && addressed(r))
        || (numtype::is_pointer_type(l) && numtype::is_pointer_type(r))
    {
        types.ops.cmp_leaf(c, NumType::I64)
    } else {
        return None;
    };
    Some(mint::Made { output_type: types.bool_, op })
}

/// The number type `l` and `r` both are; `None` where they differ or are no number type.
///
/// # Safety
/// `l` and `r` must be type nodes from the store, or null.
unsafe fn one_number_type(types: &Core, l: DyadPtr, r: DyadPtr) -> Option<NumType> {
    (l == r && is_numtype_node(types, l)).then(|| numtype::of_type_node(l))
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
            node(store, types, types.times, k, w)?
        }
        _ => return Err(ParseError::UnsupportedOperands),
    };
    node(store, types, op, lhs, offset).map(Some)
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

/// Two identities compare by reading at parse, as two types do at run; two values that read as
/// node addresses, or two pointers, compare those addresses at run. `None` hands the numeric
/// rule the rest.
fn build_identity_compare(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    c: CmpOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (l, r) =
        unsafe { (super::read::read_kind(types, lhs), super::read::read_kind(types, rhs)) };
    if l == Read::Identity && r == Read::Identity {
        // SAFETY: as above.
        let same = unsafe {
            let (l, r) = (types.through(lhs), types.through(rhs));
            let (l, r) = (super::gate::under(l), super::gate::under(r));
            l == r || (dyad::ty(l) == types.type_ && dyad::types_read_the_same(l, r))
        };
        let truth = if matches!(c, CmpOp::Eq) { same } else { !same };
        return Ok(Some(bool_mod::literal_node(store, types.bool_, truth)));
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
        // SAFETY: as above; a cell's identity read is built over a reduced dyad too.
        return unsafe { node(store, types, op, lhs, rhs) }.map(Some);
    }
    Ok(None)
}

/// The value to its right negated; a literal to the right folded it at discovery.
fn construct_negate(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    let types = p.types();
    let operand = p.operand_right(tape)?;
    // SAFETY: `operand` is the reduced dyad to the right.
    let node = unsafe { build_negate(p.store(), types, id, operand) }?;
    // SAFETY: `node` was just built over a reduced dyad.
    let node = unsafe { rational::slotted(p, node) };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// `[operand]` under negation's mint for the operand's type, which it gives back; a plain number
/// folds, and over a group each member is negated.
///
/// # Safety
/// `operand` must be a reduced dyad from the store.
unsafe fn build_negate(
    store: &mut Store,
    types: &Core,
    negate: DyadPtr,
    operand: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if let Some((connective, a, b)) = group::members(types, operand) {
        let a = build_negate(store, types, negate, a)?;
        let b = build_negate(store, types, negate, b)?;
        return group::join(store, types, connective, a, b);
    }
    if rational::is_rational_value(types, operand) {
        return mint::node(store, types, negate, &[types.rational], &[operand]);
    }
    match super::operand_of(types, operand) {
        super::Operand::Literal => {
            Ok(rational::negate(store, types.rational, types.through(operand)))
        }
        super::Operand::Concrete(nt) => {
            let t = types.numtypes[nt as usize];
            let operand = super::land(store, types, operand, t)?;
            mint::node(store, types, negate, &[t], &[operand])
        }
        super::Operand::Pointer(_) | super::Operand::NonNumeric => {
            Err(ParseError::UnsupportedOperands)
        }
    }
}

/// What negation's mint over the field type `key` gives back, that type, and runs: the negation
/// of a number type or of a run-time rational; stand-in for #462.
///
/// # Safety
/// `key` must hold type nodes from the store, or null.
unsafe fn choose_negate(_: &mut Store, types: &Core, key: &[DyadPtr]) -> Option<mint::Made> {
    let &[t] = key else {
        return None;
    };
    let op = if t == types.rational {
        types.ops.rational_neg
    } else if is_numtype_node(types, t) {
        types.ops.neg[numtype::of_type_node(t) as usize]
    } else {
        return None;
    };
    Some(mint::Made { output_type: t, op })
}

fn lower_negate(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a node of one of negation's mints, `[operand]`.
    unsafe { lw.lower_negate(node) }
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
            Family::Logic(l) => lw.lower_logic(node, l),
        }
    }
}
