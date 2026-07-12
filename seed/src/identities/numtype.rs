//! `NumType`: the seed's numeric machine types, and the type-switched arithmetic and
//! comparison the operators dispatch through.
//!
//! A binary numeric operator node is `{ty: op, value: [lhs, rhs, type]}` — the
//! resolved operand type is stored in the value slot (DESIGN ›which concrete machine
//! operation runs is resolved from the operand types‹). Run and compile read that
//! stored type and switch on it, so one `+`/`<`/… identity serves every numeric type
//! and the ~70 machine ops are a table, not ~70 files.
//!
//! Each numeric **type node** self-describes its `NumType` by a one-byte tag in its
//! own value slot (see [`tag_bytes`]/[`of_type_node`]), so neither the interpreter nor
//! the compiler needs a separate type→NumType map — the tag rides the graph.

use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};

use super::operands;

/// A numeric machine type. `#[repr(u8)]` so the discriminant is the type node's tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum NumType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

impl NumType {
    /// The tag byte a numeric type node stores in its value slot.
    pub(crate) fn tag_bytes(self) -> [u8; 1] {
        [self as u8]
    }

    /// Recover a `NumType` from a tag byte.
    fn from_tag(t: u8) -> NumType {
        use NumType::*;
        match t {
            0 => I8,
            1 => I16,
            2 => I32,
            3 => I64,
            4 => U8,
            5 => U16,
            6 => U32,
            7 => U64,
            8 => F32,
            9 => F64,
            _ => unreachable!("invalid NumType tag {t}"),
        }
    }

    /// Whether this is a floating-point type.
    pub(crate) fn is_float(self) -> bool {
        matches!(self, NumType::F32 | NumType::F64)
    }

    /// Whether this is a signed integer type (false for unsigned and floats).
    pub(crate) fn is_signed_int(self) -> bool {
        matches!(self, NumType::I8 | NumType::I16 | NumType::I32 | NumType::I64)
    }
}

/// The `NumType` a numeric type node describes (read from its value-slot tag).
///
/// # Safety
/// `type_node` must be a numeric type node registered with a [`NumType::tag_bytes`]
/// value (see `identities::i32` and its siblings).
pub(crate) unsafe fn of_type_node(type_node: DyadPtr) -> NumType {
    NumType::from_tag(*((*type_node).value as *const u8))
}

/// The type stored in a binary operator node's value slot (its third operand).
///
/// # Safety
/// `node` must be a resolved binary numeric operator node `[lhs, rhs, type]`.
pub(crate) unsafe fn stored_type(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(2)
}

/// The `NumType` a binary numeric operator resolved to (its stored operand type).
///
/// # Safety
/// As [`stored_type`] + [`of_type_node`].
unsafe fn stored_numtype(node: DyadPtr) -> NumType {
    of_type_node(stored_type(node))
}

/// The three machine arithmetic operations.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
}

/// The six machine comparisons (their result is `bool`, an i32 0/1).
#[derive(Debug, Clone, Copy)]
pub(crate) enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// Interpret the `i64` bit-containers `l`/`r` as `ty`, apply the arithmetic `op` with
/// wrapping semantics (matching the JIT), and return the result's bit-container.
fn apply_arith(op: ArithOp, ty: NumType, l: i64, r: i64) -> i64 {
    macro_rules! int {
        ($t:ty) => {{
            let (a, b) = (l as $t, r as $t);
            let v = match op {
                ArithOp::Add => a.wrapping_add(b),
                ArithOp::Sub => a.wrapping_sub(b),
                ArithOp::Mul => a.wrapping_mul(b),
            };
            v as i64
        }};
    }
    macro_rules! float {
        ($t:ty, $from:path, $bits:ty) => {{
            let (a, b): ($t, $t) = ($from(l as $bits), $from(r as $bits));
            let v = match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
            };
            v.to_bits() as i64
        }};
    }
    use NumType::*;
    match ty {
        I8 => int!(i8),
        I16 => int!(i16),
        I32 => int!(i32),
        I64 => int!(i64),
        U8 => int!(u8),
        U16 => int!(u16),
        U32 => int!(u32),
        U64 => int!(u64),
        F32 => float!(f32, f32::from_bits, u32),
        F64 => float!(f64, f64::from_bits, u64),
    }
}

/// Interpret `l`/`r` as `ty` and apply the comparison `op`, returning the i32 0/1
/// bool in an `i64`.
fn apply_compare(op: CmpOp, ty: NumType, l: i64, r: i64) -> i64 {
    macro_rules! cmp {
        ($a:expr, $b:expr) => {{
            let (a, b) = ($a, $b);
            let v = match op {
                CmpOp::Lt => a < b,
                CmpOp::Gt => a > b,
                CmpOp::Le => a <= b,
                CmpOp::Ge => a >= b,
                CmpOp::Eq => a == b,
                CmpOp::Ne => a != b,
            };
            i64::from(v as i32)
        }};
    }
    use NumType::*;
    match ty {
        I8 => cmp!(l as i8, r as i8),
        I16 => cmp!(l as i16, r as i16),
        I32 => cmp!(l as i32, r as i32),
        I64 => cmp!(l, r),
        U8 => cmp!(l as u8, r as u8),
        U16 => cmp!(l as u16, r as u16),
        U32 => cmp!(l as u32, r as u32),
        U64 => cmp!(l as u64, r as u64),
        F32 => cmp!(f32::from_bits(l as u32), f32::from_bits(r as u32)),
        F64 => cmp!(f64::from_bits(l as u64), f64::from_bits(r as u64)),
    }
}

/// Run a binary arithmetic operator: read its stored type, evaluate both operands to
/// their bit-containers, and apply the arithmetic in that type. The shared `run` for
/// `+`/`-`/`*`.
///
/// # Safety
/// `node` must be a resolved binary numeric operator node `[lhs, rhs, type]`.
pub(crate) unsafe fn eval_arith(
    rt: &mut Runtime,
    node: DyadPtr,
    op: ArithOp,
) -> Result<i64, RunError> {
    let ty = stored_numtype(node);
    let (lhs, rhs) = operands(node);
    let l = rt.run(lhs)?;
    let r = rt.run(rhs)?;
    Ok(apply_arith(op, ty, l, r))
}

/// Run a binary comparison operator: read its stored operand type, evaluate both
/// operands, and compare in that type (result is the i32 0/1 bool). The shared `run`
/// for `<`/`>`/`==`/`<=`/`>=`/`!=`.
///
/// # Safety
/// As [`eval_arith`].
pub(crate) unsafe fn eval_compare(
    rt: &mut Runtime,
    node: DyadPtr,
    op: CmpOp,
) -> Result<i64, RunError> {
    let ty = stored_numtype(node);
    let (lhs, rhs) = operands(node);
    let l = rt.run(lhs)?;
    let r = rt.run(rhs)?;
    Ok(apply_compare(op, ty, l, r))
}
