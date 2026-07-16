// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `return`: an optional early exit from the enclosing function. Its node is
//! `{ty: return, value: [value, op]}` — the punned single-operand form widened
//! so the node can reference its native leaf like every other runnable (issue
//! #44) — and run/compile evaluate the operand and yield it. Surface: prefix,
//! `return <expr>`.
//!
//! `return` is not needed to produce a value: a scope is valued by what it
//! evaluates to (its trailing expression), so `return X` and `X` coincide in tail
//! position. v1 has no control flow to exit past, so `return X` is simply the value
//! of the `( )` it sits in; the early-exit-to-the-nearest-function semantics
//! (DESIGN ›A scope's value is what it evaluates to‹) arrive with control flow.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError, Schedule};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `return`: spelling, prefix constructor, native leaf, and lowering.
/// In v1 `return` is optional (a body is valued by what it evaluates to); it is
/// kept as an explicit yield and becomes early-return once control flow lands.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        0.0,
        Assoc::Left,
        Schedule::Prefix,
        &["value", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("return", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Prefix(build));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// Build `return <operand>` as `{ty: return, value: [operand, op]}`.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    return_id: DyadPtr,
    operand: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let value = store.alloc_operands(&[operand, types.ops.return_]);
    Ok(store.alloc_raw(return_id, value))
}

/// The single operand of a `return` node (its first slot).
///
/// # Safety
/// `node` must be a `return` node `[value, op]` as [`build`] lays it out.
unsafe fn operand(node: DyadPtr) -> DyadPtr {
    *((*node).value as *const DyadPtr)
}

/// Run: evaluate the single operand and yield it.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid return node; its first slot is its operand.
    unsafe { rt.run(operand(node)) }
}

/// Lower: lower the single operand and yield it.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid return node; its first slot is its operand.
    unsafe { lw.lower(operand(node)) }
}
