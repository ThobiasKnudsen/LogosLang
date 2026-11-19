// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `rational_number`, the numeric literal's type: a reduced fraction `[num, den]`
//! (`den > 0`), two native-endian `i64`s. A literal molds exactly to a numeric type
//! where it lands (`mold_to`), or stays a rational value at run time: the same sixteen
//! bytes in a place, copied like a record, the leaves below operating on them and no
//! node made. DESIGN ›A plain number is a `rational_number`, and a number type's
//! definition converts it‹.

use super::numtype::{ArithOp, CmpOp, NumType};
use super::read::{read_kind, Dispatch, Read};
use super::{meta, Cx};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The type, ranked and associating as a number type does, and the plain number, an identity of
/// its own built as it is lexed, whose node is of the type (›Unknown spellings are two pattern
/// identities‹ [[R252]]).
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record_assoc(
        cx.store,
        meta::FRACTION_TAG,
        meta::prec::CONVERT,
        crate::parse::Assoc::Right,
    );
    let id = cx.store.alloc_head(cx.type_, record);
    // `rational_number ?` is a place of the type, `rational_number 1` the literal as a runtime
    // value.
    cx.declare("rational_number", id);
    cx.metas.insert(id, convert);
    let record = meta::record(cx.store, meta::FRACTION_TAG, meta::prec::LITERAL);
    let number = cx.store.alloc_head(cx.type_, record);
    // Unanchored: the lexer longest-matches a prefix. The span is unsigned, else `a-1` would
    // lex as `a` then `-1`: a negation before it folds into it instead.
    cx.declare_pattern(r"[0-9]+(?:\.[0-9]+)?", number);
    cx.metas.insert(number, construct);
    id
}

fn construct(
    p: &mut Parser,
    _id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let span = tape.own_text().ok_or(ParseError::BadLiteral)?;
    let types = p.types();
    let node = build(p.store(), types.rational, span)?;
    // A negation directly to the left is the negative literal, folded now: `x := -5`, `2 * -3`,
    // `i32 -5` (›`-` negates where a space stands before it and none after, or no value stands
    // to its left; any other `-` subtracts‹ [[R157]]).
    let negated =
        matches!(tape.at(-1), Some(c) if !c.constructed && c.identity(types) == types.negate);
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

pub(crate) fn build_literal(store: &mut Store, rational: DyadPtr, num: i64, den: i64) -> DyadPtr {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&num.to_ne_bytes());
    bytes[8..].copy_from_slice(&den.to_ne_bytes());
    store.alloc_blob(rational, &bytes)
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
        // A comptime name used as an operand is its binding: fold through it.
        let (lhs, rhs) = (types.through(lhs), types.through(rhs));
        if !is_literal(rational, lhs) || !is_literal(rational, rhs) {
            return Ok(None);
        }
        let (l, r) = (read_fraction(lhs), read_fraction(rhs));
        // Comptime `%` is defined over integers only; a fraction falls through to the
        // runtime path.
        if matches!(op, ArithOp::Rem) && (l.1 != 1 || r.1 != 1) {
            return Ok(None);
        }
        match fold_pair(op, l, r) {
            Some((num, den)) => Ok(Some(build_literal(store, rational, num, den))),
            None => Err(ParseError::LiteralArithmetic),
        }
    }
}

/// The exact result on two reduced fractions; `None` past `i64`, on division by zero, or
/// for `%` off the integers.
pub(crate) fn fold_pair(
    op: ArithOp,
    (n1, d1): (i64, i64),
    (n2, d2): (i64, i64),
) -> Option<(i64, i64)> {
    let (n1, d1, n2, d2) = (i128::from(n1), i128::from(d1), i128::from(n2), i128::from(d2));
    // The denominators are `i64`, so each product fits `i128`; only the add/sub of
    // the cross-products can overflow.
    let (num, den) = match op {
        ArithOp::Add => ((n1 * d2).checked_add(n2 * d1)?, d1 * d2),
        ArithOp::Sub => ((n1 * d2).checked_sub(n2 * d1)?, d1 * d2),
        ArithOp::Mul => (n1 * n2, d1 * d2),
        ArithOp::Div => {
            if n2 == 0 {
                return None;
            }
            (n1 * d2, d1 * n2)
        }
        ArithOp::Rem => {
            if d1 != 1 || d2 != 1 || n2 == 0 {
                return None;
            }
            (n1 % n2, 1)
        }
    };
    let (num, den) = reduce(num, den);
    Some((i64::try_from(num).ok()?, i64::try_from(den).ok()?))
}

/// `None` if either operand is not a rational literal.
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
        Some(compare_pair(op, read_fraction(lhs), read_fraction(rhs)))
    }
}

/// Cross-multiplies (`den > 0` keeps the direction); the products fit `i128`.
pub(crate) fn compare_pair(op: CmpOp, (n1, d1): (i64, i64), (n2, d2): (i64, i64)) -> bool {
    let l = i128::from(n1) * i128::from(d2);
    let r = i128::from(n2) * i128::from(d1);
    match op {
        CmpOp::Lt => l < r,
        CmpOp::Gt => l > r,
        CmpOp::Le => l <= r,
        CmpOp::Ge => l >= r,
        CmpOp::Eq => l == r,
        CmpOp::Ne => l != r,
    }
}

/// A literal of the type, its fraction inline, as against a place of the type, which
/// holds a value's address.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
unsafe fn is_literal(rational: DyadPtr, node: DyadPtr) -> bool {
    !node.is_null() && dyad::ty(node) == rational
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
    spell_pair(read_fraction(node))
}

/// The value at `p` as it would be spelled.
///
/// # Safety
/// As `read_at`.
pub(crate) unsafe fn spell_at(p: *const u8) -> String {
    spell_pair(read_at(p))
}

fn spell_pair((num, den): (i64, i64)) -> String {
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
    read_at(dyad::value(node))
}

/// # Safety
/// `p` must point at sixteen readable bytes.
pub(crate) unsafe fn read_at(p: *const u8) -> (i64, i64) {
    let num = std::ptr::read_unaligned(p as *const i64);
    let den = std::ptr::read_unaligned(p.add(8) as *const i64);
    (num, den)
}

/// # Safety
/// `p` must point at sixteen writable bytes.
pub(crate) unsafe fn write_at(p: *mut u8, num: i64, den: i64) {
    std::ptr::write_unaligned(p as *mut i64, num);
    std::ptr::write_unaligned(p.add(8) as *mut i64, den);
}

/// The value of `nt` the plain number is exactly, `None` where there is none. DESIGN ›A plain
/// number is a `rational_number`, and a number type's definition converts it‹.
pub(crate) fn mold_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { dyad::value(node) };
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
        // Reduced, so exact only over a power of two, with no more significant bits than the
        // float's digits.
        let n = num.unsigned_abs();
        let significant = (u64::BITS - n.leading_zeros()).saturating_sub(n.trailing_zeros());
        if !(den as u64).is_power_of_two() || significant as usize > nt.digits() {
            return None;
        }
        return Some(cast_pair(num, den, nt));
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

/// What `lossy` gives for a plain number: an integer type takes its whole part, wrapped to
/// width, a float type its nearest value; `None` for a malformed rational.
pub(crate) fn cast_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { dyad::value(node) };
    if p.is_null() {
        return None;
    }
    // SAFETY: as above, and the blob is present.
    let (num, den) = unsafe { read_fraction(node) };
    if den == 0 {
        return None;
    }
    Some(cast_pair(num, den, nt))
}

/// [`cast_to`] on a fraction, `den > 0`: a float type rounds the exact fraction once.
pub(crate) fn cast_pair(num: i64, den: i64, nt: NumType) -> i64 {
    // `|num / den|` lies in [2^-63, 2^63], inside both floats' normal range: the scaling by
    // `2^-s` is exact, and no fraction is subnormal or past the largest finite value.
    match nt {
        NumType::F32 => {
            let (m, s) = sticky_quotient(num, den, nt.digits());
            let scale =
                f32::from_bits(((f32::MAX_EXP - 1 - s) as u32) << (f32::MANTISSA_DIGITS - 1));
            i64::from((m as f32 * scale).to_bits())
        }
        NumType::F64 => {
            let (m, s) = sticky_quotient(num, den, nt.digits());
            let scale =
                f64::from_bits(((f64::MAX_EXP - 1 - s) as u64) << (f64::MANTISSA_DIGITS - 1));
            (m as f64 * scale).to_bits() as i64
        }
        // Through the shared integer wrap, so it matches a runtime `i64` to `nt` convert.
        _ => super::numtype::encode_from_i128(nt, i128::from(num / den)),
    }
}

/// `num / den` as `m / 2^s`: `|m|` is the quotient scaled to `digits + 2` or `digits + 3`
/// bits, its lowest bit set when the division leaves a remainder. That bit lies below the
/// rounding bit, so `m as` a float of `digits` rounds as the exact fraction does: to nearest,
/// ties to even.
fn sticky_quotient(num: i64, den: i64, digits: usize) -> (i64, i32) {
    if num == 0 {
        return (0, 0);
    }
    let (a, b) = (u128::from(num.unsigned_abs()), den as u128);
    let s = digits as i32 + 2 + a.leading_zeros() as i32 - b.leading_zeros() as i32;
    let (n, d) = if s >= 0 { (a << s, b) } else { (a, b << -s) };
    let m = (n / d) as i64 | i64::from(n % d != 0);
    (if num < 0 { -m } else { m }, s)
}

/// The value to the right (`Parser::type_operand`): `rational_number 1` hands the literal on
/// as a run-time value, its bytes the value's; `rational_number r` passes a rational value as it
/// is, and a value of any other type is refused, as it is where it lands in a rational place. With
/// no value to the right the type stands as itself.
fn convert(
    p: &mut Parser,
    _id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let types = p.types();
    let Some(operand) = p.type_operand(tape, 1)? else {
        return Ok(Constructed::Decline);
    };
    let value = p.as_operand(operand)?;
    // SAFETY: `value` is a reduced dyad from the store.
    let node = unsafe {
        let read = types.through(value);
        if is_literal(types.rational, read) {
            Ok(super::by_copy::build_out(p.store(), types, read))
        } else {
            super::land(p.store(), types, value, types.rational)
        }
    }
    .map_err(|e| p.fail_at(operand.start, e))?;
    tape.remove(1);
    tape.place(node);
    Ok(Constructed::Placed)
}

/// A rational arithmetic node runs into a slot of its own, since its value is sixteen
/// bytes and no node is made for it: `node`, or each member of a group, wrapped as a
/// `result` over a fresh place. Anything else is left as it stands.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn slotted(p: &mut Parser, node: DyadPtr) -> DyadPtr {
    let types = p.types();
    if let Some((_, a, b)) = super::group::members(types, node) {
        let group = types.through(node);
        let ops = dyad::value(group) as *mut DyadPtr;
        *ops = slotted(p, a);
        *ops.add(1) = slotted(p, b);
        return node;
    }
    let arith = match read_kind(types, node) {
        Read::Executable(Dispatch::Leaf(leaf)) => types.ops.gives_rational(leaf),
        _ => false,
    };
    if !arith {
        return node;
    }
    super::by_copy::build_result(p.store(), types, std::ptr::null_mut(), node, types.rational)
}

/// A node that gives back a `rational_number` when it runs; a bare literal is not one, it
/// molds where it lands.
///
/// # Safety
/// `node` must be null or a valid dyad from the store.
pub(crate) unsafe fn is_rational_value(types: &Core, node: DyadPtr) -> bool {
    let read = types.through(node);
    !read.is_null()
        && !is_literal(types.rational, read)
        && super::read::output_type(types, node) == types.rational
}

/// The fraction a rational operand yields at run: a literal's own, else the sixteen
/// bytes at the address the node yields.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn value_of(rt: &mut Runtime, node: DyadPtr) -> Result<(i64, i64), RunError> {
    let read = rt.through(node);
    if is_literal(rt.types().rational, read) {
        return Ok(read_fraction(read));
    }
    let addr = rt.run(node)? as usize as *const u8;
    if addr.is_null() {
        return Err(RunError::Uninitialized);
    }
    let (num, den) = read_at(addr);
    // A place is zeroed until written, and no fraction has a zero denominator.
    if den == 0 {
        return Err(RunError::Uninitialized);
    }
    Ok((num, den))
}

/// Run the arithmetic node `node`, `[lhs, rhs]` or a negation's `[operand]` under its mint, its
/// result written to `dest`.
///
/// # Safety
/// `node` must be a rational arithmetic node from the store; `dest` sixteen writable bytes.
pub(crate) unsafe fn run_into(
    rt: &mut Runtime,
    node: DyadPtr,
    dest: *mut u8,
) -> Result<(), RunError> {
    let ops = dyad::value(node) as *const DyadPtr;
    let leaf = super::mint::operation_of(dyad::ty(node)).ok_or(RunError::NoLeaf)?;
    let (num, den) = if leaf == rt.types().ops.rational_neg {
        let (num, den) = value_of(rt, *ops)?;
        (num.checked_neg().ok_or(RunError::RationalArithmetic)?, den)
    } else {
        let op = rt.types().ops.rational_arith_op_of(leaf).ok_or(RunError::NoLeaf)?;
        let l = value_of(rt, *ops)?;
        let r = value_of(rt, *ops.add(1))?;
        fold_pair(op, l, r).ok_or(RunError::RationalArithmetic)?
    };
    write_at(dest, num, den);
    Ok(())
}

/// An arithmetic node runs through its `result` slot, never bare.
fn run_arith_unslotted(_rt: &mut Runtime, _node: DyadPtr) -> Result<i64, RunError> {
    Err(RunError::NoWholeRead)
}

/// # Safety
/// `node` must be a binary operator node whose first two slots are its operands.
unsafe fn compare_at_run(rt: &mut Runtime, node: DyadPtr, op: CmpOp) -> Result<i64, RunError> {
    let ops = dyad::value(node) as *const DyadPtr;
    let l = value_of(rt, *ops)?;
    let r = value_of(rt, *ops.add(1))?;
    Ok(i64::from(compare_pair(op, l, r)))
}

macro_rules! rational_cmp {
    ($name:ident, $op:expr) => {
        fn $name(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
            // SAFETY: `node` is a binary operator node built over rational operands.
            unsafe { compare_at_run(rt, node, $op) }
        }
    };
}
rational_cmp!(run_lt, CmpOp::Lt);
rational_cmp!(run_gt, CmpOp::Gt);
rational_cmp!(run_le, CmpOp::Le);
rational_cmp!(run_ge, CmpOp::Ge);
rational_cmp!(run_eq, CmpOp::Eq);
rational_cmp!(run_ne, CmpOp::Ne);

/// Indexed as `ArithOp`: one refusal each, the leaves distinct by node.
pub(crate) const ARITH_RUNS: [crate::run::RunFn; 5] = [run_arith_unslotted; 5];
pub(crate) const NEG_RUN: crate::run::RunFn = run_arith_unslotted;
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

    #[test]
    fn a_fraction_rounds_once_to_the_nearest_float() {
        let f32_of = |num, den| f32::from_bits(cast_pair(num, den, NumType::F32) as u32);
        let f64_of = |num, den| f64::from_bits(cast_pair(num, den, NumType::F64) as u64);
        let up = 1.0 + f32::EPSILON;
        // 1 + 2^-24 + 2^-60 and 1 + 3 * 2^-24 - 2^-60: through an `f64` each became a tie.
        assert_eq!(f32_of((1 << 60) + (1 << 36) + 1, 1 << 60), up);
        assert_eq!(f32_of(-(1 << 60) - (1 << 36) - 1, 1 << 60), -up);
        assert_eq!(f32_of((1 << 60) + (3 << 36) - 1, 1 << 60), up);
        assert_eq!(f32_of((1 << 24) + 1, 1 << 24), 1.0);
        assert_eq!(f32_of((1 << 24) + 3, 1 << 24), up + f32::EPSILON);
        // Parts past 2^53, which an `f64` rounds before the division does.
        assert_eq!(f64_of((1 << 55) + 2, 3), 12009599006321324.0);
        assert_eq!(f64_of(-(1 << 55) - 2, 3), -12009599006321324.0);
        assert_eq!(f64_of(1, (1 << 53) + 1).to_bits(), (1.0 / (1u64 << 53) as f64).to_bits() - 1);
        assert_eq!(f32_of(i64::MIN, 1), i64::MIN as f32);
        assert_eq!(f32_of(i64::MAX, 1), -(i64::MIN as f32));
        assert_eq!(f64_of(1, i64::MAX), 1.0 / -(i64::MIN as f64));
        assert_eq!(f64_of(0, 7).to_bits(), 0);
    }
}
