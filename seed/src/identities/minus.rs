// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `-`: subtraction. Like `+` (see [`crate::identities::plus`]), it stores its
//! resolved operand type in the value slot `{ty: -, value: [lhs, rhs, type]}` and
//! run/compile switch on it. Same precedence as `+`, left-associative.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_arith, ArithOp};
use super::{rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `-`: spelling, precedence (same as `+`, left-associative), and its
/// type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("-", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 2.0, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs - rhs`: resolve the operand type and store it as the third operand.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    minus: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math); otherwise resolve and build.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Sub, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ops = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&ops);
    Ok(store.alloc_raw(minus, value))
}

/// Run: subtract in the stored operand type.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `-` application `[lhs, rhs, type]`.
    unsafe { eval_arith(rt, node, ArithOp::Sub) }
}

/// Lower: emit the machine subtraction for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `-` application `[lhs, rhs, type]`.
    unsafe { lw.lower_arith(node, ArithOp::Sub) }
}
