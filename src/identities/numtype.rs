// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `NumType`, the seed's numeric machine type, and the type-switched arithmetic,
//! comparison, cast, read and write helpers the concrete ops' shims share. A numeric
//! type node self-describes its `NumType` by the kind byte of its record, so the tag
//! rides the graph.

use cranelift_codegen::ir::types;

use crate::dyad::DyadPtr;

use super::Cx;

/// `#[repr(u8)]` so the discriminant is the type node's tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NumType {
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
    pub(crate) fn from_tag(t: u8) -> NumType {
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

    pub(crate) fn spelling(self) -> &'static str {
        use NumType::*;
        match self {
            I8 => "i8",
            I16 => "i16",
            I32 => "i32",
            I64 => "i64",
            U8 => "u8",
            U16 => "u16",
            U32 => "u32",
            U64 => "u64",
            F32 => "f32",
            F64 => "f64",
        }
    }

    pub(crate) fn is_float(self) -> bool {
        matches!(self, NumType::F32 | NumType::F64)
    }

    pub(crate) fn is_signed_int(self) -> bool {
        matches!(self, NumType::I8 | NumType::I16 | NumType::I32 | NumType::I64)
    }

    pub(crate) fn bytes(self) -> usize {
        use NumType::*;
        match self {
            I8 | U8 => 1,
            I16 | U16 => 2,
            I32 | U32 | F32 => 4,
            I64 | U64 | F64 => 8,
        }
    }

    /// The saturation sentinel division by zero yields; an unsigned maximum is all-ones,
    /// which is -1 sign-narrowed.
    pub(crate) fn max_imm(self) -> i64 {
        use NumType::*;
        match self {
            I8 => i64::from(i8::MAX),
            I16 => i64::from(i16::MAX),
            I32 => i64::from(i32::MAX),
            I64 => i64::MAX,
            U8 | U16 | U32 | U64 => -1,
            F32 | F64 => unreachable!("integer saturation only"),
        }
    }

    pub(crate) fn min_imm(self) -> i64 {
        use NumType::*;
        match self {
            I8 => i64::from(i8::MIN),
            I16 => i64::from(i16::MIN),
            I32 => i64::from(i32::MIN),
            I64 => i64::MIN,
            _ => unreachable!("signed integers only"),
        }
    }

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

pub(crate) fn register_type(cx: &mut Cx, spelling: &str, nt: NumType) -> DyadPtr {
    let record = super::meta::record(cx.store, nt as u8, super::meta::prec::APPLY);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare(spelling, id);
    cx.metas.insert(id, construct);
    id
}

/// Juxtaposition: a directly following rational literal (or `- <rational>`) commits
/// exactly to this type; a following `(` is the conversion `i32(x)`; anything else
/// declines the right and the type stands as a value, so `f(i32, 3)` passes the type.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, crate::parse::ParseError> {
    let types = p.types();
    // Runs at discovery, before the loop has lexed past the right cell. A sign there
    // belongs to the literal after it, which folds it at its own discovery, so that
    // literal is lexed too.
    let mut right = p.cell_at(tape, 1)?;
    if matches!(right, Some(c) if !c.constructed && c.identity(types) == types.minus) {
        p.cell_at(tape, 2)?;
        right = tape.at(1).copied();
    }
    let node = match right {
        Some(c)
            // SAFETY: a constructed, non-bracket cell holds a node from the store.
            if c.constructed && !c.is_bracket() && unsafe { (*c.dyad).ty } == types.rational =>
        {
            let l = c.dyad;
            tape.remove(1);
            // SAFETY: `l` is rational-typed by the guard; `id` is this numeric type's node.
            unsafe { super::commit_literal_to(p.store(), types, l, id) }?
        }
        // `i32(x)`: the bracket is this type's to read, a conversion, never `(`'s call.
        Some(c) if c.is_bracket() => {
            let scope = c.dyad;
            // SAFETY: `scope` is the bracket's node from the store.
            let args = unsafe { p.args_of(scope) };
            tape.remove(1);
            // SAFETY: `id` is this numeric type's node; `args` are reduced dyads.
            unsafe { p.build_call(id, &args) }?
        }
        _ => p.stand_as_value(tape, id),
    };
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// One past every `NumType` discriminant, so a void type node is told apart by its tag alone.
pub(crate) const VOID_TAG: u8 = 10;

/// No lowering: `void` appears only as a `->` return type.
pub(crate) fn register_void(cx: &mut Cx) -> DyadPtr {
    let record = super::meta::record(cx.store, VOID_TAG, super::meta::prec::INERT);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("void", id);
    id
}

/// # Safety
/// `type_node` must be null or a valid type node from the store.
pub(crate) unsafe fn is_void_type(type_node: DyadPtr) -> bool {
    if type_node.is_null() {
        return false;
    }
    let v = (*type_node).value;
    !v.is_null() && *(v as *const u8) == VOID_TAG
}

pub(crate) const STRING_TAG: u8 = 11;

pub(crate) const COMMENT_TAG: u8 = 12;

/// # Safety
/// `type_node` must be null or a valid type node from the store.
pub(crate) unsafe fn is_comment_type(type_node: DyadPtr) -> bool {
    if type_node.is_null() {
        return false;
    }
    let v = (*type_node).value;
    !v.is_null() && *(v as *const u8) == COMMENT_TAG
}

/// A pointer type's record carries its pointee as the payload.
pub(crate) const ADDR_TAG: u8 = 13;

/// # Safety
/// `type_node` must be null or a valid type node from the store.
pub(crate) unsafe fn is_pointer_type(type_node: DyadPtr) -> bool {
    if type_node.is_null() {
        return false;
    }
    let v = (*type_node).value;
    !v.is_null() && *(v as *const u8) == ADDR_TAG
}

/// # Safety
/// `type_node` must be a pointer type node (`is_pointer_type`).
pub(crate) unsafe fn pointee_of(type_node: DyadPtr) -> DyadPtr {
    let p = (*type_node).value.add(super::meta::PAYLOAD_OFF);
    std::ptr::read_unaligned(p as *const DyadPtr)
}

/// A numeric or pointer type; `void`, text, a record type, and a null type are not scalars.
///
/// # Safety
/// `type_node` must be null or a record-carrying type node from the store.
pub(crate) unsafe fn is_scalar_type(type_node: DyadPtr) -> bool {
    if type_node.is_null() {
        return false;
    }
    let v = (*type_node).value;
    if v.is_null() {
        return false;
    }
    let tag = *(v as *const u8);
    tag < VOID_TAG || tag == ADDR_TAG
}

/// Into the `i64` container: sign or zero-extended per signedness, raw bits for floats.
///
/// # Safety
/// `type_node` is a valid type node; `slot` points at a value of that type's width.
pub(crate) unsafe fn read_scalar(type_node: DyadPtr, slot: *const u8) -> i64 {
    read_scalar_nt(of_type_node(type_node), slot)
}

/// # Safety
/// `slot` points at storage of `nt`'s width.
pub(crate) unsafe fn read_scalar_nt(nt: NumType, slot: *const u8) -> i64 {
    use std::ptr::read_unaligned as rd;
    use NumType::*;
    match nt {
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

/// # Safety
/// `type_node` is a valid type node; `slot` points at storage of that type's width.
pub(crate) unsafe fn write_scalar(type_node: DyadPtr, slot: *mut u8, bits: i64) {
    write_scalar_nt(of_type_node(type_node), slot, bits)
}

/// The form a concrete store op uses, its type baked at registration.
///
/// # Safety
/// `slot` points at storage of `nt`'s width.
pub(crate) unsafe fn write_scalar_nt(nt: NumType, slot: *mut u8, bits: i64) {
    use std::ptr::write_unaligned as wr;
    use NumType::*;
    match nt {
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

/// A pointer type reads and writes as its 8-byte address (`U64`), which gives every
/// width-driven path pointer behaviour.
///
/// # Safety
/// `type_node` must be a numeric or pointer type node.
pub(crate) unsafe fn of_type_node(type_node: DyadPtr) -> NumType {
    let tag = *((*type_node).value as *const u8);
    if tag == ADDR_TAG {
        return NumType::U64;
    }
    NumType::from_tag(tag)
}

/// A conversion is `[operand, from, to, op]`; this is `to`.
///
/// # Safety
/// `node` must be a conversion node from `convert::build_convert`.
pub(crate) unsafe fn stored_type(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr).add(2)
}

/// Integer `Div`/`Rem` are total: a zero divisor yields the type's MAX, the signed
/// MIN/-1 overflow saturates to MAX, and MIN % -1 is 0. Float `Rem` is rejected at
/// parse (Cranelift has no float remainder).
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl ArithOp {
    /// The concrete-op registration loop's index (a const generic cannot be an enum on stable
    /// Rust).
    pub(crate) fn from_tag(t: u8) -> ArithOp {
        use ArithOp::*;
        match t {
            0 => Add,
            1 => Sub,
            2 => Mul,
            3 => Div,
            4 => Rem,
            _ => unreachable!("invalid ArithOp tag {t}"),
        }
    }
}

/// The result is `bool`, an i32 0/1.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    pub(crate) fn from_tag(t: u8) -> CmpOp {
        use CmpOp::*;
        match t {
            0 => Lt,
            1 => Gt,
            2 => Le,
            3 => Ge,
            4 => Eq,
            5 => Ne,
            _ => unreachable!("invalid CmpOp tag {t}"),
        }
    }
}

/// Wrapping semantics, matching the JIT.
pub(crate) fn apply_arith(op: ArithOp, logos: NumType, l: i64, r: i64) -> i64 {
    macro_rules! int {
        ($t:ty) => {{
            let (a, b) = (l as $t, r as $t);
            let v = match op {
                ArithOp::Add => a.wrapping_add(b),
                ArithOp::Sub => a.wrapping_sub(b),
                ArithOp::Mul => a.wrapping_mul(b),
                ArithOp::Div => {
                    if b == 0 {
                        <$t>::MAX
                    } else {
                        a.checked_div(b).unwrap_or(<$t>::MAX)
                    }
                }
                ArithOp::Rem => {
                    if b == 0 {
                        <$t>::MAX
                    } else {
                        a.checked_rem(b).unwrap_or(0)
                    }
                }
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
                ArithOp::Div => a / b,
                ArithOp::Rem => unreachable!("float % is rejected at parse"),
            };
            v.to_bits() as i64
        }};
    }
    use NumType::*;
    match logos {
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

/// `None` where the sum does not fit: a `for` counter's step ends the loop there instead
/// of wrapping. A float sum always fits.
pub(crate) fn checked_add(logos: NumType, l: i64, r: i64) -> Option<i64> {
    macro_rules! int {
        ($t:ty) => {
            (l as $t).checked_add(r as $t).map(|v| v as i64)
        };
    }
    use NumType::*;
    match logos {
        I8 => int!(i8),
        I16 => int!(i16),
        I32 => int!(i32),
        I64 => int!(i64),
        U8 => int!(u8),
        U16 => int!(u16),
        U32 => int!(u32),
        U64 => int!(u64),
        F32 | F64 => Some(apply_arith(ArithOp::Add, logos, l, r)),
    }
}

pub(crate) fn apply_compare(op: CmpOp, logos: NumType, l: i64, r: i64) -> i64 {
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
    match logos {
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

/// Rust `as` semantics. The compiler's `emit_cast` must stay bit-for-bit identical: the
/// interpreter is the compiler's oracle.
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
    // Every integer width fits an i128 exactly.
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
