// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `and`: short-circuiting conjunction over two `bool`s, or, over two
//! non-booleans, a group the consuming operator distributes over.
//! DESIGN ›The proof layer‹

use crate::Core;
use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{bool_mod, meta, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{bool_literal_value, is_bool_result, Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::AND,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("and", id);
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The node is `[lhs, rhs, op]`; a group's op slot is null.
pub(super) fn build(
    store: &mut Store,
    types: &Core,
    and: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lb, rb) = unsafe { (is_bool_result(types, lhs), is_bool_result(types, rhs)) };
    if lb != rb {
        return Err(ParseError::NonBoolOperands);
    }
    if !lb {
        let value = store.alloc_operands(&[lhs, rhs, std::ptr::null_mut()]);
        return Ok(store.alloc_raw(and, value));
    }
    // Two literals fold now: what keeps a comptime chain comptime.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let literals = unsafe { (bool_literal_value(types, lhs), bool_literal_value(types, rhs)) };
    if let (Some(a), Some(b)) = literals {
        return Ok(bool_mod::literal_node(store, types.bool_, a && b));
    }
    let value = store.alloc_operands(&[lhs, rhs, types.ops.and_]);
    Ok(store.alloc_raw(and, value))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `and` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        if rt.run(lhs)? != 0 {
            rt.run(rhs)
        } else {
            Ok(0)
        }
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `and` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_and(lhs, rhs)
    }
}
