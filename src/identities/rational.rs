// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `rational_number`, the numeric literal's type: a reduced fraction `[num, den]`
//! (`den > 0`), two native-endian `i64`s. A literal molds exactly to a numeric type
//! where it lands (`mold_to`), or stays a rational value at run time, where the leaves
//! below operate on it. DESIGN ›Numeric literals are uncommitted until context classifies them‹.

use super::numtype::{ArithOp, CmpOp, NumType};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::FRACTION_TAG, meta::prec::LITERAL);
    let id = cx.store.alloc_raw(cx.type_, record);
    // Unanchored: the lexer longest-matches a prefix. The span is unsigned: `-` is
    // always the operator, else `a-1` would lex as `a` then `-1`.
    cx.declare(r"[0-9]+(?:\.[0-9]+)?", id);
    // `rational_number ?` is a place of the type, `rational_number 1` the literal as a runtime
    // value.
    cx.declare("rational_number", id);
    cx.metas.insert(id, construct);
    id
}

fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let span = tape.own_text().ok_or(ParseError::BadLiteral)?;
    if !span.starts_with(|c: char| c.is_ascii_digit()) {
        // The type's name, or a cell folded to the type: the conversion form, or the type
        // standing as itself.
        return convert(p, tape);
    }
    let node = build(p.store(), id, span)?;
    // A `-` directly to the left with no completed operand before it is the negative
    // literal, folded now: `x := -5`, `2 * -3`, while `a - 5` keeps its subtraction.
    let types = p.types();
    let negated = matches!(tape.at(-1), Some(c) if !c.constructed && c.identity(types) == types.minus)
        && !tape.at(-2).is_some_and(|c| p.is_operand_cell(c));
    let node = if negated {
        tape.remove(-1);
        // SAFETY: `node` is the literal just built.
        unsafe { negate(p.store(), types.rational, node) }
    } else {
        node
    };
    tape.place(node);
    Ok(Constructed::Placed)
}

/// A well-formed decimal always builds; whether it computes as an `i32` is a use-site question.
pub(crate) fn build(
    store: &mut Store,
    rational: DyadPtr,
    span: &str,
) -> Result<DyadPtr, ParseError> {
    let (num, den) = parse_fraction(span).ok_or(ParseError::BadLiteral)?;
    Ok(build_literal(store, rational, num, den))
}

fn build_literal(store: &mut Store, rational: DyadPtr, num: i64, den: i64) -> DyadPtr {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&num.to_ne_bytes());
    bytes[8..].copy_from_slice(&den.to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    store.alloc_raw(rational, value)
}

/// `Ok(None)` if either operand is not a rational literal; `UncomputableLiteral` if the
/// exact result overflows the `i64` num/den.
pub(crate) fn fold_arith(
    store: &mut Store,
    types: &Core,
    op: ArithOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<Option<DyadPtr>, ParseError> {
    let rational = types.rational;
    // SAFETY: `lhs`/`rhs` are valid dyads; a rational-typed one holds a `[num, den]` blob.
    unsafe {
        // A comptime binding used as an operand is its record: fold through it.
        let (lhs, rhs) = (types.through(lhs), types.through(rhs));
        if !is_literal(rational, lhs) || !is_literal(rational, rhs) {
            return Ok(None);
        }
        let (n1, d1) = read_fraction(lhs);
        let (n2, d2) = read_fraction(rhs);
        let (n1, d1, n2, d2) = (i128::from(n1), i128::from(d1), i128::from(n2), i128::from(d2));
        // The denominators are `i64`, so each product fits `i128`; only the add/sub of
        // the cross-products can overflow.
        let (num, den) = match op {
            ArithOp::Add => {
                ((n1 * d2).checked_add(n2 * d1).ok_or(ParseError::UncomputableLiteral)?, d1 * d2)
            }
            ArithOp::Sub => {
                ((n1 * d2).checked_sub(n2 * d1).ok_or(ParseError::UncomputableLiteral)?, d1 * d2)
            }
            ArithOp::Mul => (n1 * n2, d1 * d2),
            ArithOp::Div => {
                if n2 == 0 {
                    return Err(ParseError::UncomputableLiteral);
                }
                (n1 * d2, d1 * n2)
            }
            // Comptime `%` is defined over integers only; a fraction falls through to the
            // runtime path.
            ArithOp::Rem => {
                if d1 != 1 || d2 != 1 {
                    return Ok(None);
                }
                if n2 == 0 {
                    return Err(ParseError::UncomputableLiteral);
                }
                (n1 % n2, 1)
            }
        };
        let (num, den) = reduce(num, den);
        match (i64::try_from(num), i64::try_from(den)) {
            (Ok(num), Ok(den)) => Ok(Some(build_literal(store, rational, num, den))),
            _ => Err(ParseError::UncomputableLiteral),
        }
    }
}

/// Cross-multiplies (`den > 0` keeps the direction); the products fit `i128`.
pub(crate) fn compare_literals(
    types: &Core,
    op: CmpOp,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Option<bool> {
    let rational = types.rational;
    // SAFETY: `lhs`/`rhs` are valid dyads; a rational-typed one holds a `[num, den]` blob.
    unsafe {
        let (lhs, rhs) = (types.through(lhs), types.through(rhs));
        if !is_literal(rational, lhs) || !is_literal(rational, rhs) {
            return None;
        }
        let (n1, d1) = read_fraction(lhs);
        let (n2, d2) = read_fraction(rhs);
        let l = i128::from(n1) * i128::from(d2);
        let r = i128::from(n2) * i128::from(d1);
        Some(match op {
            CmpOp::Lt => l < r,
            CmpOp::Gt => l > r,
            CmpOp::Le => l <= r,
            CmpOp::Ge => l >= r,
            CmpOp::Eq => l == r,
            CmpOp::Ne => l != r,
        })
    }
}

/// A literal of the type, its fraction inline, as against a place of the type, which
/// holds a value's address.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
unsafe fn is_literal(rational: DyadPtr, node: DyadPtr) -> bool {
    !node.is_null() && (*node).ty == rational && !crate::dyad::is_place((*node).value)
}

fn reduce(mut num: i128, mut den: i128) -> (i128, i128) {
    if den < 0 {
        num = -num;
        den = -den;
    }
    if num == 0 {
        return (0, 1);
    }
    let g = gcd128(num.unsigned_abs(), den as u128);
    (num / g as i128, den / g as i128)
}

fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// `None` on `i64` overflow or malformed input, so the caller reports a clean error.
fn parse_fraction(span: &str) -> Option<(i64, i64)> {
    let (int_part, frac_part) = match span.split_once('.') {
        Some((i, f)) => (i, f),
        None => (span, ""),
    };
    if int_part.is_empty() || int_part.bytes().any(|b| !b.is_ascii_digit()) {
        return None;
    }
    if frac_part.bytes().any(|b| !b.is_ascii_digit()) {
        return None;
    }
    let mut num: i64 = int_part.parse().ok()?;
    let mut den: i64 = 1;
    for b in frac_part.bytes() {
        let d = i64::from(b - b'0');
        num = num.checked_mul(10)?.checked_add(d)?;
        den = den.checked_mul(10)?;
    }
    let g = gcd(num.unsigned_abs(), den as u64);
    if g > 1 {
        num /= g as i64;
        den /= g as i64;
    }
    Some((num, den))
}

/// # Safety
/// `node` must be a rational literal from the store.
pub(crate) unsafe fn negate(store: &mut Store, rational: DyadPtr, node: DyadPtr) -> DyadPtr {
    let (num, den) = read_fraction(node);
    build_literal(store, rational, -num, den)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// The literal as it would be spelled: `5`, `-3`, `5/2`.
///
/// # Safety
/// As `read_fraction`.
pub(crate) unsafe fn spell(node: DyadPtr) -> String {
    let (num, den) = read_fraction(node);
    if den == 1 {
        num.to_string()
    } else {
        format!("{num}/{den}")
    }
}

/// # Safety
/// `node` must be a rational literal from `build`: its `value` points at the 16-byte
/// `[num, den]` blob.
unsafe fn read_fraction(node: DyadPtr) -> (i64, i64) {
    let p = (*node).value;
    let num = std::ptr::read_unaligned(p as *const i64);
    let den = std::ptr::read_unaligned(p.add(8) as *const i64);
    (num, den)
}

/// An integer target needs an exact in-range integer; a float target takes `num/den`;
/// `None` where there is no exact value.
pub(crate) fn mold_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { (*node).value };
    if p.is_null() {
        return None;
    }
    // SAFETY: as above, and the blob is present.
    let (num, den) = unsafe { read_fraction(node) };
    if den == 0 {
        return None;
    }
    use NumType::*;
    if nt.is_float() {
        let v = num as f64 / den as f64;
        return match nt {
            F32 => Some(i64::from((v as f32).to_bits())),
            F64 => Some(v.to_bits() as i64),
            _ => None,
        };
    }
    if num % den != 0 {
        return None;
    }
    let q = num / den;
    match nt {
        I8 => i8::try_from(q).ok().map(i64::from),
        I16 => i16::try_from(q).ok().map(i64::from),
        I32 => i32::try_from(q).ok().map(i64::from),
        I64 => Some(q),
        U8 => u8::try_from(q).ok().map(i64::from),
        U16 => u16::try_from(q).ok().map(i64::from),
        U32 => u32::try_from(q).ok().map(i64::from),
        U64 => u64::try_from(q).ok().map(|v| v as i64),
        F32 | F64 => None,
    }
}

/// `mold_to` at `i32`, for the bare-literal paths.
pub(crate) fn mold(node: DyadPtr) -> Option<i32> {
    mold_to(node, NumType::I32).map(|b| b as i32)
}

/// `as` semantics, for the `T(literal)` conversion: an integer target takes the truncated
/// integer part then wraps to width; `None` for a malformed rational.
pub(crate) fn cast_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { (*node).value };
    if p.is_null() {
        return None;
    }
    // SAFETY: as above, and the blob is present.
    let (num, den) = unsafe { read_fraction(node) };
    if den == 0 {
        return None;
    }
    if nt.is_float() {
        let v = num as f64 / den as f64;
        return Some(match nt {
            NumType::F32 => i64::from((v as f32).to_bits()),
            NumType::F64 => v.to_bits() as i64,
            _ => unreachable!("nt is a float here"),
        });
    }
    // Through the shared cast, so it matches a runtime `i64` to `nt` convert.
    Some(super::numtype::apply_cast(NumType::I64, nt, num / den))
}

/// `rational_number 1` boxes the literal as a runtime value, `rational_number r` passes a
/// rational value as it is; with nothing of the kind to the right the type stands as
/// itself. A concrete number is not converted.
fn convert(p: &mut Parser, tape: &mut ParsingTape) -> Result<Constructed, ParseError> {
    let types = p.types();
    let Some(right) = p.cell_at(tape, 1)? else {
        return Ok(Constructed::Decline);
    };
    if !right.constructed {
        return Ok(Constructed::Decline);
    }
    // SAFETY: a constructed cell's dyad is a node from the store.
    let value = unsafe { rational_operand(p.store(), types, right.dyad) };
    match value {
        Some(v) => {
            tape.remove(1);
            tape.place(v);
            Ok(Constructed::Placed)
        }
        None => Ok(Constructed::Decline),
    }
}

/// A rational value is the address of a literal node, carried in the `i64` container. A
/// literal is boxed as a `dyad` view of itself; the operators over such values are
/// interpreted only.
pub(crate) fn box_literal(store: &mut Store, types: &Core, lit: DyadPtr) -> DyadPtr {
    store.alloc_raw(types.dyad_, lit.cast())
}

/// A place of the type, a boxed literal, an operator node over rationals, or a call whose
/// output is the type; a bare literal is not one, it molds where it lands.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_rational_value(types: &Core, node: DyadPtr) -> bool {
    use super::read::{read_kind, Dispatch, Read};
    let node = types.through(node);
    if node.is_null() {
        return false;
    }
    let ty = (*node).ty;
    if ty == types.ran_ {
        return is_rational_value(types, super::ran::expr_of(types, node));
    }
    if ty == types.scope {
        return crate::parse::last_sequence_expr(node)
            .is_some_and(|last| is_rational_value(types, last));
    }
    match read_kind(types, node) {
        Read::Container(t) => t == types.rational,
        Read::Address => {
            let viewed = (*node).value as DyadPtr;
            !viewed.is_null() && (*viewed).ty == types.rational
        }
        Read::Executable(Dispatch::Call(f)) => {
            let fields = (*f).value as *const DyadPtr;
            !fields.is_null() && *fields.add(crate::parse::FN_OUTPUT) == types.rational
        }
        Read::Executable(Dispatch::Leaf(leaf)) => types.ops.is_rational_leaf(leaf),
        _ => false,
    }
}

/// A literal boxed, a rational value as it is, anything else `None`.
///
/// # Safety
/// As `is_rational_value`.
pub(crate) unsafe fn rational_operand(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
) -> Option<DyadPtr> {
    let read = types.through(node);
    if !read.is_null() && (*read).ty == types.rational && !crate::dyad::is_place((*read).value) {
        return Some(box_literal(store, types, read));
    }
    is_rational_value(types, node).then_some(node)
}

/// # Safety
/// `node` must be a binary operator node whose first two slots are its operands.
unsafe fn operand_values(rt: &mut Runtime, node: DyadPtr) -> Result<(DyadPtr, DyadPtr), RunError> {
    let ops = (*node).value as *const DyadPtr;
    let l = rt.run(*ops)? as DyadPtr;
    let r = rt.run(*ops.add(1))? as DyadPtr;
    if l.is_null() || r.is_null() {
        return Err(RunError::Uninitialized);
    }
    Ok((l, r))
}

/// # Safety
/// As `operand_values`.
unsafe fn fold_at_run(rt: &mut Runtime, node: DyadPtr, op: ArithOp) -> Result<i64, RunError> {
    let (l, r) = operand_values(rt, node)?;
    let types: *const Core = rt.types();
    // SAFETY: the `Core` outlives the runtime that borrowed it.
    let types = &*types;
    match fold_arith(rt.store(), types, op, l, r) {
        Ok(Some(v)) => Ok(v as i64),
        _ => Err(RunError::UncomputableLiteral),
    }
}

/// # Safety
/// As `operand_values`.
unsafe fn compare_at_run(rt: &mut Runtime, node: DyadPtr, op: CmpOp) -> Result<i64, RunError> {
    let (l, r) = operand_values(rt, node)?;
    let types: *const Core = rt.types();
    // SAFETY: the `Core` outlives the runtime that borrowed it.
    let types = &*types;
    compare_literals(types, op, l, r).map(i64::from).ok_or(RunError::UncomputableLiteral)
}

macro_rules! rational_leaf {
    ($name:ident, $call:ident, $op:expr) => {
        fn $name(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
            // SAFETY: `node` is a binary operator node built over rational operands.
            unsafe { $call(rt, node, $op) }
        }
    };
}
rational_leaf!(run_add, fold_at_run, ArithOp::Add);
rational_leaf!(run_sub, fold_at_run, ArithOp::Sub);
rational_leaf!(run_mul, fold_at_run, ArithOp::Mul);
rational_leaf!(run_div, fold_at_run, ArithOp::Div);
rational_leaf!(run_rem, fold_at_run, ArithOp::Rem);
rational_leaf!(run_lt, compare_at_run, CmpOp::Lt);
rational_leaf!(run_gt, compare_at_run, CmpOp::Gt);
rational_leaf!(run_le, compare_at_run, CmpOp::Le);
rational_leaf!(run_ge, compare_at_run, CmpOp::Ge);
rational_leaf!(run_eq, compare_at_run, CmpOp::Eq);
rational_leaf!(run_ne, compare_at_run, CmpOp::Ne);

/// Indexed as `ArithOp`.
pub(crate) const ARITH_RUNS: [crate::run::RunFn; 5] = [run_add, run_sub, run_mul, run_div, run_rem];
/// Indexed as `CmpOp`.
pub(crate) const CMP_RUNS: [crate::run::RunFn; 6] =
    [run_lt, run_gt, run_le, run_ge, run_eq, run_ne];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integers_decimals_and_reduces() {
        assert_eq!(parse_fraction("42"), Some((42, 1)));
        assert_eq!(parse_fraction("3.14"), Some((157, 50)));
        assert_eq!(parse_fraction("2.5"), Some((5, 2)));
        assert_eq!(parse_fraction("6.0"), Some((6, 1)));
        assert_eq!(parse_fraction("0"), Some((0, 1)));
        assert_eq!(parse_fraction("0.0"), Some((0, 1)));
        // The span is unsigned: a leading `-` is the operator, never the literal.
        assert_eq!(parse_fraction("-42"), None);
    }

    #[test]
    fn rejects_overflowing_literals() {
        assert_eq!(parse_fraction("99999999999999999999999999"), None);
    }
}
