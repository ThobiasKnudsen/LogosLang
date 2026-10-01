// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `or`: disjunction over two `bool`s, both sides run, or, over two
//! non-booleans, a group the consuming operator distributes over, as `and`.
//! DESIGN ›`and` and `or` run both sides; `if` is the one identity that skips‹,
//! ›`and`/`or` on non-booleans build a group; every operator on a group applies to each member‹

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
        meta::prec::OR,
        Assoc::Left,
        &["lhs", "rhs", "op", "output_type"],
    );
    let id = cx.store.alloc_head(cx.type_, record);
    cx.declare("or", id);
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// The node is `[lhs, rhs, op, output_type]`; a group's op slot and output are null, as
/// `and`'s are.
pub(super) fn build(
    store: &mut Store,
    types: &Core,
    or: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (lb, rb) = unsafe { (is_bool_result(types, lhs), is_bool_result(types, rhs)) };
    if lb != rb {
        return Err(ParseError::NonBoolOperands);
    }
    if !lb {
        return Ok(store.alloc_words(or, &[lhs, rhs, std::ptr::null_mut(), std::ptr::null_mut()]));
    }
    // Two literals fold now: what keeps a comptime chain comptime.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let literals = unsafe { (bool_literal_value(types, lhs), bool_literal_value(types, rhs)) };
    if let (Some(a), Some(b)) = literals {
        return Ok(bool_mod::literal_node(store, types.bool_, a || b));
    }
    Ok(store.alloc_words(or, &[lhs, rhs, types.ops.or_, types.bool_]))
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        let l = rt.run(lhs)?;
        let r = rt.run(rhs)?;
        Ok((l != 0 || r != 0) as i64)
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_or(lhs, rhs)
    }
}
