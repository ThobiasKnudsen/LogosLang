//! `rational_number`: the numeric-literal carrier. A data type whose value is an
//! actual rational — a reduced fraction `num / den` (`den > 0`) — so `3.14` is a
//! first-class literal (`157 / 50`), not just whole numbers. Integer literals are
//! the `den == 1` case.
//!
//! A rational only becomes a machine number when it is *molded* to a concrete type
//! at use (DESIGN ›Numeric literals are uncommitted until context types them‹). v1
//! has a single concrete numeric type, `i32`, so molding means: if the fraction is
//! an exact integer in `i32` range, that integer; otherwise there is no way to
//! compute the literal, reported as [`RunError::UncomputableLiteral`] /
//! [`CompileError::UncomputableLiteral`] rather than a crash. Parsing a decimal
//! therefore always succeeds (it is a valid rational); only *computing* `3.14` as
//! an integer fails, cleanly.
//!
//! Storage: the value points at 16 bytes, two native-endian `i64`s `[num, den]`.

use cranelift_codegen::ir::Value;

use super::numtype::NumType;
use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Construct, ParseError};
use crate::store::Store;

/// Register `rational_number`: its spelling (integers or decimals), literal
/// constructor, and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.type_, std::ptr::null_mut());
    // An optional sign, digits, and an optional fractional part. Unanchored: the
    // lexer longest-matches a prefix of the remaining input.
    cx.trie.insert(r"-?[0-9]+(?:\.[0-9]+)?", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Atom(build));
    cx.lower.insert(id, lower);
    id
}

/// Build a rational literal `{ty: rational, value: [num, den]}` from its span,
/// parsing the decimal text into a reduced fraction. A malformed or out-of-`i64`
/// -range span is a [`ParseError::BadLiteral`]; a well-formed decimal always builds
/// (whether it can later be computed as an `i32` is a use-site question).
fn build(store: &mut Store, rational: DyadPtr, span: &str) -> Result<DyadPtr, ParseError> {
    let (num, den) = parse_fraction(span).ok_or(ParseError::BadLiteral)?;
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&num.to_ne_bytes());
    bytes[8..].copy_from_slice(&den.to_ne_bytes());
    let value = store.alloc_bytes(&bytes);
    Ok(store.alloc_raw(rational, value))
}

/// Lower a rational literal to an `i32` immediate, or fail if it has no exact `i32`
/// value (a fraction or an out-of-range integer) — the same outcome the
/// interpreter reaches, so compiled and interpreted agree.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    match mold(node) {
        Some(v) => Ok(lw.const_i32(v)),
        None => Err(CompileError::UncomputableLiteral),
    }
}

/// Parse `-?[0-9]+(?:\.[0-9]+)?` into a reduced fraction `(num, den)` with
/// `den > 0`. Returns `None` on overflow of `i64` (a huge literal) or malformed
/// input, so the caller reports a clean error instead of panicking.
fn parse_fraction(span: &str) -> Option<(i64, i64)> {
    let neg = span.starts_with('-');
    let body = span.strip_prefix('-').unwrap_or(span);
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
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
    if neg {
        num = num.checked_neg()?;
    }
    let g = gcd(num.unsigned_abs(), den as u64);
    if g > 1 {
        num /= g as i64;
        den /= g as i64;
    }
    Some((num, den))
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
/// `i64` bit-container. Integer types require an exact integer (`den` divides `num`)
/// in range; float types take `num/den` as the float's bits. Returns `None` if there
/// is no exact value (a decimal to an int, or an out-of-range integer) — which the
/// run/compile paths turn into `UncomputableLiteral`, and which parse-time committing
/// turns into a literal-does-not-fit error. A null value slot also yields `None`.
pub(crate) fn mold_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { (*node).value };
    if p.is_null() {
        return None;
    }
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
/// explicit-conversion counterpart to [`mold_to`]'s exact commit, for a `type(literal)`
/// constructor. An integer target takes the fraction's truncated-toward-zero integer
/// part, then wraps to width; a float target takes `num/den`. Returns the value's `i64`
/// bit-container, or `None` for a malformed rational (null or zero denominator).
pub(crate) fn cast_to(node: DyadPtr, nt: NumType) -> Option<i64> {
    // SAFETY: called only on rational-typed nodes, whose value is the [num, den] blob.
    let p = unsafe { (*node).value };
    if p.is_null() {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integers_decimals_and_reduces() {
        assert_eq!(parse_fraction("42"), Some((42, 1)));
        assert_eq!(parse_fraction("-42"), Some((-42, 1)));
        assert_eq!(parse_fraction("3.14"), Some((157, 50))); // 314/100 reduced
        assert_eq!(parse_fraction("-2.5"), Some((-5, 2))); // -25/10 reduced
        assert_eq!(parse_fraction("6.0"), Some((6, 1))); // trailing zero reduces to whole
        assert_eq!(parse_fraction("0"), Some((0, 1)));
        assert_eq!(parse_fraction("0.0"), Some((0, 1)));
    }

    #[test]
    fn rejects_overflowing_literals() {
        // Past i64: no fraction, a clean None (caller -> ParseError::BadLiteral).
        assert_eq!(parse_fraction("99999999999999999999999999"), None);
    }
}
