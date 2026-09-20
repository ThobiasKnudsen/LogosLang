// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Numeric conversion, `i32(a)`, `f64(x)`: one shared `convert` identity for
//! every scalar cast, the node `[operand, from, to, op]` with the source and
//! target as numtype nodes. The only cross-type path; there is no implicit coercion.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{apply_cast, of_type_node, NumType};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// No spelling: a conversion is built from the `logos(value)` surface.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["operand", "from", "to", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

pub(crate) fn build_convert(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    from: DyadPtr,
    to: DyadPtr,
) -> DyadPtr {
    let value = store.alloc_operands(&[operand, from, to, types.ops.convert_]);
    store.alloc_raw(types.convert, value)
}

/// # Safety
/// `node` must be a conversion node `[operand, from, to, op]`.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, NumType, NumType) {
    let p = (*node).value as *const DyadPtr;
    (*p, of_type_node(*p.add(1)), of_type_node(*p.add(2)))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = rt.run(operand)?;
        Ok(apply_cast(from, to, v))
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = lw.lower(operand)?;
        Ok(lw.emit_cast(from, to, v))
    }
}
