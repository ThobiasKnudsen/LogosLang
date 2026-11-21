// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Conversion, written (`i64 (a)`, `i32 lossy (x)`, `@i32 &a`) or not: one shared `convert`
//! identity for every conversion between number types and between pointer types, the node
//! `[operand]` under its mint for the source and target types, number types or `rational_number`
//! as the source for a rational value. A pointer keeps its address; only its type changes.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::numtype::{apply_cast, is_pointer_type, of_type_node};
use super::{meta, mint, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// No spelling: a conversion is built from the `logos(value)` surface.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, meta::prec::INERT, Assoc::Left, &["operand"]);
    let id = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(id, lower);
    cx.choosers.insert(id, choose);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// # Safety
/// `operand` must be a reduced dyad, `from` and `to` type nodes, all from the store.
pub(crate) unsafe fn build_convert(
    store: &mut Store,
    types: &Core,
    operand: DyadPtr,
    from: DyadPtr,
    to: DyadPtr,
) -> DyadPtr {
    mint::node_of_any(store, types, types.convert, &[from, to], &[operand])
}

/// What `convert`'s mint over the field types `key`, the source and the target, gives back, the
/// target, and runs; stand-in for #462.
///
/// # Safety
/// `key` must hold type nodes from the store.
unsafe fn choose(_: &mut Store, types: &Core, key: &[DyadPtr]) -> Option<mint::Made> {
    let &[_, to] = key else {
        return None;
    };
    Some(mint::Made { output_type: to, op: types.ops.convert_ })
}

/// `(operand, from, to)`.
///
/// # Safety
/// `node` must be a conversion node [`build_convert`] built.
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let operand = *(dyad::value(node) as *const DyadPtr);
    match mint::key_of(dyad::ty(node)) {
        Some(&[from, to]) => (operand, from, to),
        _ => unreachable!("a conversion is typed by its mint for its source and target"),
    }
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid conversion node; a rational value is the address of its
    // sixteen bytes.
    unsafe {
        let (operand, from, to) = parts(node);
        let v = rt.run(operand)?;
        if is_pointer_type(to) {
            return Ok(v);
        }
        let to = of_type_node(to);
        if from == rt.types().rational {
            let p = v as usize as *const u8;
            if p.is_null() {
                return Err(RunError::Uninitialized);
            }
            let (num, den) = super::rational::read_at(p);
            if den == 0 {
                return Err(RunError::Uninitialized);
            }
            return Ok(super::rational::cast_pair(num, den, to));
        }
        apply_cast(of_type_node(from), to, v).ok_or(RunError::NoWholeNumber)
    }
}

/// A rational value is interpreted only, as every operator over one is.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid conversion node.
    unsafe {
        let (operand, from, to) = parts(node);
        if from == lw.types().rational {
            return Err(CompileError::NotLowerable(node));
        }
        let v = lw.lower(operand)?;
        if is_pointer_type(to) {
            return Ok(v);
        }
        lw.emit_cast(of_type_node(from), of_type_node(to), v)
    }
}
