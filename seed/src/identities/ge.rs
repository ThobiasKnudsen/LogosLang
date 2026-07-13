// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `>=`: greater-than-or-equal. Like `<` (see [`crate::identities::lt`]), it stores its
//! resolved operand type in the value slot and run/compile switch on it; result is
//! `bool`. The trie longest-matches `>=` over `>`.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_compare, CmpOp};
use super::{bool_mod, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `>=`: spelling, precedence (relational, left-associative), and its
/// type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert(">=", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 1.5, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs >= rhs`: resolve the operand type and store it as the third operand.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    ge: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now to a `bool` literal; otherwise resolve and build.
    if let Some(v) = rational::compare_literals(types.rational, CmpOp::Ge, lhs, rhs) {
        return Ok(bool_mod::literal_node(store, types.bool_, v));
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ops = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&ops);
    Ok(store.alloc_raw(ge, value))
}

/// Run: compare in the stored operand type.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `>=` application `[lhs, rhs, type]`.
    unsafe { eval_compare(rt, node, CmpOp::Ge) }
}

/// Lower: emit the machine comparison for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `>=` application `[lhs, rhs, type]`.
    unsafe { lw.lower_compare(node, CmpOp::Ge) }
}
