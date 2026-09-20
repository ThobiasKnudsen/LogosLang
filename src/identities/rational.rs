// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `rational_number`: the numeric-literal carrier. A data type whose value is an
//! actual rational — a reduced fraction `num / den` (`den > 0`) — so `3.14` is a
//! first-class literal (`157 / 50`), not just whole numbers. Integer literals are
//! the `den == 1` case.
//!
//! A rational only becomes a machine number when it is *molded* to a concrete type
//! at use (DESIGN ›Numeric literals are uncommitted until context classifies them‹):
//! [`mold_to`] commits it exactly to any numeric width (an integer target requires
//! an exact in-range integer; a float target takes `num/den`), and a literal that
//! never lands in a typed slot defaults to `i32` when read ([`mold`]). A literal
//! with no exact value in its target is reported as
//! [`crate::run::RunError::UncomputableLiteral`] /
//! [`CompileError::UncomputableLiteral`] rather than a crash. Parsing a decimal
//! therefore always succeeds (it is a valid rational); only *computing* `3.14` as
//! an integer fails, cleanly.
//!
//! Storage: the value points at 16 bytes, two native-endian `i64`s `[num, den]`.

use super::numtype::{ArithOp, CmpOp, NumType};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Constructed, ParseError, Parser, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// Register `rational_number`: its spelling (integers or decimals), literal
/// constructor, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::record(cx.store, meta::FRACTION_TAG, meta::prec::LITERAL);
    let id = cx.store.alloc_raw(cx.type_, record);
    // Digits and an optional fractional part. Unanchored: the lexer longest-matches
    // a prefix of the remaining input. The span is unsigned — `-` is always the
    // operator (else `a-1` would lex as `a` then the literal `-1`); a negative
    // literal is the prefix `-` negating the literal at parse time ([`negate`]).
    cx.declare(r"[0-9]+(?:\.[0-9]+)?", id);
    // The type's own name (ruled 20 September 2026: the type for number
    // literals is `rational_number`): `rational_number ?` is a place of it,
    // `rational_number 1` the literal as a runtime value ([`convert`]).
    cx.declare("rational_number", id);
    cx.metas.insert(id, construct);
    id
}

/// The literal's constructor: read the matched span off the cursor token,
/// build the leaf, and place it over its own token.
fn construct(
    p: &mut Parser,
    id: DyadPtr,
    tape: &mut ParsingTape,
) -> Result<Constructed, ParseError> {
    let span = tape.own_text().ok_or(ParseError::BadLiteral)?;
    if !span.starts_with(|c: char| c.is_ascii_digit()) {
        // The type's name, or a cell a run body's `this.output` folded to
        // the type: the conversion form, or the type standing as itself.
        return convert(p, tape);
    }
    let node = build(p.store(), id, span)?;
    // A `-` directly to the left with no completed operand before it is the
    // negative literal, folded now, at discovery (DESIGN ›Numeric literals‹:
    // "`-3` still folds into the literal"): `x := -5`, `f(-1)`, `2 * -3`,
    // `i32 -5` — while `a - 5` keeps its subtraction.
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

/// Build a rational literal `{type: rational, value: [num, den]}` from its span,
/// parsing the decimal text into a reduced fraction. A malformed or out-of-`i64`
/// -range span is a [`ParseError::BadLiteral`]; a well-formed decimal always builds
/// (whether it can later be computed as an `i32` is a use-site question). Also the
/// parser's direct path for the negated-literal and range-endpoint services.
pub(crate) fn build(
    store: &mut Store,
    rational: DyadPtr,
    span: &str,
) -> Result<DyadPtr, ParseError> {
    let (num, den) = parse_fraction(span).ok_or(ParseError::BadLiteral)?;
    Ok(build_literal(store, rational, num, den))
}

/// Build a rational literal node `{type: rational, value: [num, den]}` from an already
/// reduced fraction (`den > 0`).
fn build_literal(store: &mut Store, rational: DyadPtr, num: i64, den: i64) -> DyadPtr {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&num.to_ne_bytes());
    bytes[8..].copy_from_slice(&den.to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    store.alloc_raw(rational, value)
}

/// Fold `op` over two rational **literals** into a new rational literal by exact
/// fraction arithmetic (DESIGN ›both-uncommitted operands stay `rational`, committing
/// only when context type them‹). Returns `Ok(None)` if either operand is not a
/// rational literal (so the operator builds a normal node instead), or
/// [`ParseError::UncomputableLiteral`] if the exact result overflows the seed's `i64`
/// num/den (its rationals are `i64` fractions; arbitrary precision is later work).
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
        // `d1`,`d2` come from `i64` denominators, so each product fits `i128`; only the
        // add/sub of the two cross-products can overflow, which `checked_*` catches.
        let (num, den) = match op {
            ArithOp::Add => {
                ((n1 * d2).checked_add(n2 * d1).ok_or(ParseError::UncomputableLiteral)?, d1 * d2)
            }
            ArithOp::Sub => {
                ((n1 * d2).checked_sub(n2 * d1).ok_or(ParseError::UncomputableLiteral)?, d1 * d2)
            }
            ArithOp::Mul => (n1 * n2, d1 * d2),
            // Exact fraction division (`1 / 3` IS one third); a zero divisor has
            // no comptime value.
            ArithOp::Div => {
                if n2 == 0 {
                    return Err(ParseError::UncomputableLiteral);
                }
                (n1 * d2, d1 * n2)
            }
            // Comptime `%` is defined over integers (den == 1); a fraction falls
            // through to the committed runtime path. A zero divisor has no
            // comptime value.
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

/// Compare two rational **literals** exactly, returning the boolean or `None` if either
/// operand is not a rational literal. Cross-multiplies (`den > 0` keeps the direction);
/// the products fit `i128`, so this never overflows.
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

/// Reduce an `i128` fraction to lowest terms with a positive denominator.
/// Whether `node` is a literal of the type — its fraction inline — and not
/// a place of the type, which holds a value's address (#133 slice 8, part 4).
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

/// Greatest common divisor over `u128` (Euclid); `gcd128(0, d) == d`.
fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Parse `[0-9]+(?:\.[0-9]+)?` into a reduced fraction `(num, den)` with
/// `den > 0`. The span is unsigned (`-` is always the operator; a negative literal
/// is built by [`negate`]). Returns `None` on overflow of `i64` (a huge literal) or
/// malformed input, so the caller reports a clean error instead of panicking.
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

/// Build the negation of a rational literal as a new literal node — the prefix `-`
/// (always an operator; the literal regex is unsigned) applied to a numeric
/// literal at parse time.
///
/// # Safety
/// `node` must be a rational literal from the store (its value the `[num, den]`
/// blob).
pub(crate) unsafe fn negate(store: &mut Store, rational: DyadPtr, node: DyadPtr) -> DyadPtr {
    let (num, den) = read_fraction(node);
    build_literal(store, rational, -num, den)
}

/// Greatest common divisor (Euclid); `gcd(0, d) == d`.
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Read a rational node's stored fraction `(num, den)`.
///
/// The literal as it would be spelled: `5`, `-3`, `5/2`. For messages that
/// name a literal the user wrote; a fraction prints as the reduced pair the
/// node carries.
///
/// # Safety
/// As [`read_fraction`].
pub(crate) unsafe fn spell(node: DyadPtr) -> String {
    let (num, den) = read_fraction(node);
    if den == 1 {
        num.to_string()
    } else {
        format!("{num}/{den}")
    }
}

/// # Safety
/// `node` must be a rational literal built by [`build`]: its `value` points at the
/// 16-byte `[num, den]` blob.
unsafe fn read_fraction(node: DyadPtr) -> (i64, i64) {
    let p = (*node).value;
    let num = std::ptr::read_unaligned(p as *const i64);
    let den = std::ptr::read_unaligned(p.add(8) as *const i64);
    (num, den)
}

/// Mold a rational literal to a concrete numeric type `nt`, returning the value's
/// `i64` bit-container. Integer logos require an exact integer (`den` divides `num`)
/// in range; float logos take `num/den` as the float's bits. Returns `None` if there
/// is no exact value (a decimal to an int, or an out-of-range integer) — which the
/// run/compile paths turn into `UncomputableLiteral`, and which parse-time committing
/// turns into a literal-does-not-fit error. A null value slot also yields `None`.
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

/// Mold a rational literal to a concrete `i32`, if it has one. The `i32`-typed shim
/// over [`mold_to`], kept for the bare-literal run/compile paths.
pub(crate) fn mold(node: DyadPtr) -> Option<i32> {
    mold_to(node, NumType::I32).map(|b| b as i32)
}

/// Cast a rational literal to `nt` with truncating/wrapping `as` semantics — the
/// explicit-conversion counterpart to [`mold_to`]'s exact commit, for a `logos(literal)`
/// constructor. An integer target takes the fraction's truncated-toward-zero integer
/// part, then wraps to width; a float target takes `num/den`. Returns the value's `i64`
/// bit-container, or `None` for a malformed rational (null or zero denominator).
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
    // Integer target: the truncated-toward-zero integer part (`den > 0`), cast to the
    // target width through the shared cast so it matches a runtime `i64`→`nt` convert.
    Some(super::numtype::apply_cast(NumType::I64, nt, num / den))
}

/// The type applied to a value (#133 slice 8, part 4): `rational_number 1`
/// yields the literal boxed as a runtime rational value ([`box_literal`]),
/// `rational_number r` a rational value as it is, and with nothing of the
/// kind to its right the type stands as itself, so `rational_number ?` is a
/// place of the type. A concrete number is not converted here: crossing
/// into a rational at run time is not in the seed.
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

/// A rational as a runtime value (#133 slice 8, part 4; DESIGN ›Numeric
/// literals are uncommitted until context classifies them‹: "the operation
/// is carried exactly over the number's reduced fraction … and the result
/// *stays* `rational_number`"): the address of a literal node, carried in
/// the `i64` container as a type value is. A literal standing where a
/// rational value is wanted is boxed as a `dyad` view of itself, whose run
/// is that address; a place of type `rational_number` holds one
/// ([`super::read::place_layout`]); the operators over such values are the
/// leaves below, interpreted only — the compiler refuses a rational place,
/// since DESIGN defers arbitrary precision.
pub(crate) fn box_literal(store: &mut Store, types: &Core, lit: DyadPtr) -> DyadPtr {
    store.alloc_raw(types.dyad_, lit.cast())
}

/// Whether `node` reads as a rational value at run time: a place of the
/// type, a boxed literal, an operator node over rationals, or a call whose
/// output is the type. A bare literal is not one: it molds where it lands.
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

/// `node` as an operand of a rational operation: a literal boxed, a
/// rational value as it is, anything else `None`.
///
/// # Safety
/// As [`is_rational_value`].
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

/// The two operands of a rational operator node, read as the addresses they
/// carry.
///
/// # Safety
/// `node` must be a binary operator node whose first two slots are its
/// operands, as [`super::binary`] builds.
unsafe fn operand_values(rt: &mut Runtime, node: DyadPtr) -> Result<(DyadPtr, DyadPtr), RunError> {
    let ops = (*node).value as *const DyadPtr;
    let l = rt.run(*ops)? as DyadPtr;
    let r = rt.run(*ops.add(1))? as DyadPtr;
    if l.is_null() || r.is_null() {
        return Err(RunError::Uninitialized);
    }
    Ok((l, r))
}

/// An arithmetic leaf over rational values: fold the two as the parser
/// folds two literals, and yield the result's address.
///
/// # Safety
/// As [`operand_values`].
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

/// A comparison leaf over rational values.
///
/// # Safety
/// As [`operand_values`].
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
            // SAFETY: `node` is a binary operator node the family builder
            // constructed over rational operands.
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

/// The run of each arithmetic operator over rationals, indexed as
/// [`ArithOp`] is.
pub(crate) const ARITH_RUNS: [crate::run::RunFn; 5] = [run_add, run_sub, run_mul, run_div, run_rem];
/// The run of each comparison over rationals, indexed as [`CmpOp`] is.
pub(crate) const CMP_RUNS: [crate::run::RunFn; 6] =
    [run_lt, run_gt, run_le, run_ge, run_eq, run_ne];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integers_decimals_and_reduces() {
        assert_eq!(parse_fraction("42"), Some((42, 1)));
        assert_eq!(parse_fraction("3.14"), Some((157, 50))); // 314/100 reduced
        assert_eq!(parse_fraction("2.5"), Some((5, 2))); // 25/10 reduced
        assert_eq!(parse_fraction("6.0"), Some((6, 1))); // trailing zero reduces to whole
        assert_eq!(parse_fraction("0"), Some((0, 1)));
        assert_eq!(parse_fraction("0.0"), Some((0, 1)));
        // The span is unsigned: a leading `-` is the operator, never the literal.
        assert_eq!(parse_fraction("-42"), None);
    }

    #[test]
    fn rejects_overflowing_literals() {
        // Past i64: no fraction, a clean None (caller -> ParseError::BadLiteral).
        assert_eq!(parse_fraction("99999999999999999999999999"), None);
    }
}
