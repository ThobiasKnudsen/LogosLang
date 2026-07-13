// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `return`: an optional early exit from the enclosing function. It is a function
//! whose single operand is its `value`; run and compile evaluate that operand and
//! yield it. Surface: prefix, `return <expr>`.
//!
//! `return` is not needed to produce a value: a scope is valued by what it
//! evaluates to (its trailing expression), so `return X` and `X` coincide in tail
//! position. v1 has no control flow to exit past, so `return X` is simply the value
//! of the `( )` it sits in; the early-exit-to-the-nearest-function semantics
//! (DESIGN ›A scope's value is what it evaluates to‹) arrive with control flow.

use cranelift_codegen::ir::Value;

use super::Cx;
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Construct, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `return`: spelling, prefix constructor, run bcode, and lowering. In v1
/// `return` is optional (a body is valued by what it evaluates to); it is kept as
/// an explicit yield and becomes early-return once control flow lands.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("return", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Prefix(build));
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `return <operand>` as `{ty: return, value: operand}`.
fn build(store: &mut Store, return_id: DyadPtr, operand: DyadPtr) -> Result<DyadPtr, ParseError> {
    Ok(store.alloc_raw(return_id, operand.cast()))
}

/// Run: evaluate the single operand (the node's `value`) and yield it.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid return dyad; its `value` is its operand node.
    unsafe {
        let operand = (*node).value.cast::<crate::dyad::Dyad>();
        rt.run(operand)
    }
}

/// Lower: lower the single operand and yield it.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid return dyad; its `value` is its operand node.
    unsafe {
        let operand = (*node).value.cast::<crate::dyad::Dyad>();
        lw.lower(operand)
    }
}
