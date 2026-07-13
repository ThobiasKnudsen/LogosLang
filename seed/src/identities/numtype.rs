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

use cranelift_codegen::ir::{types, Value};

use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::run::{RunError, Runtime};

use super::{operands, Cx};

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

    /// The storage width of this `NumType` in bytes.
    pub(crate) fn bytes(self) -> usize {
        use NumType::*;
        match self {
            I8 | U8 => 1,
            I16 | U16 => 2,
            I32 | U32 | F32 => 4,
            I64 | U64 | F64 => 8,
        }
    }

    /// The Cranelift type a value of this `NumType` computes in.
    pub(crate) fn cranelift_type(self) -> types::Type {
        use NumType::*;
        match self {
            I8 | U8 => types::I8,
            I16 | U16 => types::I16,
            I32 | U32 => types::I32,
            I64 | U64 => types::I64,
            F32 => types::F32,
            F64 => types::F64,
        }
    }
}

/// Register a numeric type node: its spelling (so it resolves as a type name), its
/// `NumType` tag stored in the value slot (self-describing, so run/compile recover the
/// type from the graph), and the shared numeric-variable lowering [`lower_var`]. The
/// interpreter reads its values through the type's width (see [`read_scalar`]).
pub(crate) fn register_type(cx: &mut Cx, spelling: &str, nt: NumType) -> DyadPtr {
    let tag = cx.store.alloc_bytes(&nt.tag_bytes());
    let id = cx.store.alloc_raw(cx.type_, tag);
    cx.trie.insert(spelling, IdContext::new(id, cx.root_scope));
    cx.lower.insert(id, lower_var);
    id
}

/// The `NumType` of a type node, or `I32` for a fixed-width scalar type without a
/// `NumType` tag (e.g. `bool`, physically an i32).
///
/// # Safety
/// `type_node` must be a valid type node from the store.
pub(crate) unsafe fn numtype_of_type(type_node: DyadPtr) -> NumType {
    if (*type_node).value.is_null() {
        NumType::I32
    } else {
        of_type_node(type_node)
    }
}

/// Read the scalar stored at `slot`, typed by `type_node`, into the `i64`
/// bit-container the interpreter computes in (sign/zero-extended for ints per
/// signedness; the raw float bits for `f32`/`f64`).
///
/// # Safety
/// `type_node` is a valid type node; `slot` points at a value of that type's width.
pub(crate) unsafe fn read_scalar(type_node: DyadPtr, slot: *const u8) -> i64 {
    use std::ptr::read_unaligned as rd;
    use NumType::*;
    match numtype_of_type(type_node) {
        I8 => i64::from(rd(slot as *const i8)),
        I16 => i64::from(rd(slot as *const i16)),
        I32 => i64::from(rd(slot as *const i32)),
        I64 => rd(slot as *const i64),
        U8 => i64::from(rd(slot)),
        U16 => i64::from(rd(slot as *const u16)),
        U32 => i64::from(rd(slot as *const u32)),
        U64 => rd(slot as *const u64) as i64,
        F32 => i64::from(rd(slot as *const u32)),
        F64 => rd(slot as *const u64) as i64,
    }
}

/// Write the `i64` bit-container `bits` to `slot` at `type_node`'s width — the storage
/// dual of [`read_scalar`], so a write then a read round-trips.
///
/// # Safety
/// `type_node` is a valid type node; `slot` points at storage of that type's width.
pub(crate) unsafe fn write_scalar(type_node: DyadPtr, slot: *mut u8, bits: i64) {
    use std::ptr::write_unaligned as wr;
    use NumType::*;
    match numtype_of_type(type_node) {
        I8 => wr(slot as *mut i8, bits as i8),
        I16 => wr(slot as *mut i16, bits as i16),
        I32 => wr(slot as *mut i32, bits as i32),
        I64 => wr(slot as *mut i64, bits),
        U8 => wr(slot, bits as u8),
        U16 => wr(slot as *mut u16, bits as u16),
        U32 => wr(slot as *mut u32, bits as u32),
        U64 => wr(slot as *mut u64, bits as u64),
        F32 => wr(slot as *mut u32, bits as u32),
        F64 => wr(slot as *mut u64, bits as u64),
    }
}

/// Lower a numeric variable/value: load it from its baked storage at its type's width.
/// The shared lowering rule (a [`crate::compile::LowerFn`]) for every numeric type
/// node. Guards a null address, mirroring the interpreter's `BadValue`.
pub(crate) fn lower_var(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a numeric variable node from the store.
    let addr = unsafe { (*node).value };
    if addr.is_null() {
        return Err(CompileError::BadValue);
    }
    let ct = unsafe { of_type_node((*node).ty) }.cranelift_type();
    Ok(lw.load(ct, addr))
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

/// Cast the `i64` bit-container `v`, holding a `from`-typed value, to `to`, applying
/// Rust `as` semantics: int↔int wrap/extend, int→float round-to-nearest, float→int
/// saturate (NaN→0), float↔float round. Returns the result's bit-container. This is the
/// interpreter's cast; the compiler's [`crate::compile::Lowerer::emit_cast`] must stay
/// bit-for-bit identical, since the interpreter is the compiler's oracle.
pub(crate) fn apply_cast(from: NumType, to: NumType, v: i64) -> i64 {
    use NumType::*;
    if from.is_float() {
        let f = match from {
            F32 => f64::from(f32::from_bits(v as u32)),
            F64 => f64::from_bits(v as u64),
            _ => unreachable!("from is a float here"),
        };
        return encode_from_f64(to, f);
    }
    // Decode the source integer to an exact `i128` (every int width fits), reading the
    // container at the source's width and signedness.
    let i: i128 = match from {
        I8 => i128::from(v as i8),
        I16 => i128::from(v as i16),
        I32 => i128::from(v as i32),
        I64 => i128::from(v),
        U8 => i128::from(v as u8),
        U16 => i128::from(v as u16),
        U32 => i128::from(v as u32),
        U64 => i128::from(v as u64),
        _ => unreachable!("from is an int here"),
    };
    encode_from_i128(to, i)
}

/// Encode an exact integer into `to`'s bit-container (Rust `as` from `i128`: an int
/// target wraps to its width, a float target rounds to nearest).
fn encode_from_i128(to: NumType, i: i128) -> i64 {
    use NumType::*;
    match to {
        I8 => i64::from(i as i8),
        I16 => i64::from(i as i16),
        I32 => i64::from(i as i32),
        I64 => i as i64,
        U8 => i64::from(i as u8),
        U16 => i64::from(i as u16),
        U32 => i64::from(i as u32),
        U64 => (i as u64) as i64,
        F32 => i64::from((i as f32).to_bits()),
        F64 => (i as f64).to_bits() as i64,
    }
}

/// Encode a float into `to`'s bit-container (Rust `as` from `f64`: an int target
/// saturates with NaN→0, an `f32` target rounds to nearest).
fn encode_from_f64(to: NumType, f: f64) -> i64 {
    use NumType::*;
    match to {
        I8 => i64::from(f as i8),
        I16 => i64::from(f as i16),
        I32 => i64::from(f as i32),
        I64 => f as i64,
        U8 => i64::from(f as u8),
        U16 => i64::from(f as u16),
        U32 => i64::from(f as u32),
        U64 => (f as u64) as i64,
        F32 => i64::from((f as f32).to_bits()),
        F64 => f.to_bits() as i64,
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
