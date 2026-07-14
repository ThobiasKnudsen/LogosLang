// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Numeric conversion (`i32(a)`, `f64(x)`, …): a numeric type node acting as a
//! constructor over a value (DESIGN ›numeric conversion is the type constructor
//! consuming a value‹). One shared `convert` identity carries every scalar cast; a
//! conversion node is `{ty: convert, value: [operand, from, to]}`, with the source and
//! target types stored as the second and third operands (self-describing numtype nodes,
//! read via their tag). Run applies Rust `as` ([`apply_cast`]); compile emits the
//! matching Cranelift op ([`Lowerer::emit_cast`]). This is the only cross-type path;
//! there is no implicit coercion. The parser builds these from the `type(value)`
//! constructor surface (see [`super::build_cast`]), folding a literal operand directly
//! and expanding a runtime operand into a conversion node.

use cranelift_codegen::ir::Value;

use super::numtype::{apply_cast, of_type_node, NumType};
use super::{meta, Cx};
use crate::parse::Assoc;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register the shared `convert` identity: fn-typed, so run/compile dispatch to it as
/// an operation (like `+`), with its run native and lowering. It has no spelling — a
/// conversion is built from the `type(value)` surface, not resolved from a token.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, 0.0, Assoc::Left, &["operand", "from", "to"]);
    let id = cx.store.alloc_raw(cx.fn_type, record);
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `{ty: convert, value: [operand, from, to]}`: the conversion of `operand`
/// (a value of type `from`) to type `to`. `from`/`to` are numtype nodes.
pub(crate) fn build_convert(
    store: &mut Store,
    convert: DyadPtr,
    operand: DyadPtr,
    from: DyadPtr,
    to: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[operand, from, to]);
    store.alloc_raw(convert, value)
}

/// The `(operand, from, to)` of a conversion node.
///
/// # Safety
/// `node` must be a conversion node `{ty: convert, value: [operand, from, to]}`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, NumType, NumType) {
    let p = (*node).value as *const DyadPtr;
    (*p, of_type_node(*p.add(1)), of_type_node(*p.add(2)))
}

/// Run: evaluate the operand, then cast its bit-container from `from` to `to`.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = rt.run(operand)?;
        Ok(apply_cast(from, to, v))
    }
}

/// Lower: lower the operand, then emit the machine cast from `from` to `to`.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = lw.lower(operand)?;
        Ok(lw.emit_cast(from, to, v))
    }
}
