// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `and`: short-circuiting logical conjunction over `bool`s. Both operands must be
//! `bool` (comparisons, `bool` values, or nested logical results); the result is a
//! `bool`. It short-circuits: when the left operand is false the right is not
//! evaluated, so run branches and compile lowers a two-way branch (`if a then b else
//! false`, [`crate::compile::Lowerer::lower_and`]). It binds looser than the
//! comparisons and tighter than `or`. One concrete native serves it (bool has one
//! width): the node is `{ty: and, value: [lhs, rhs, and_native]}` and run jumps
//! through the op slot (issue #44).

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{bool_mod, meta, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{
    bool_literal_value, is_bool_result, Assoc, CoreTypes, ParseError, Schedule,
};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `and`: spelling and parse precedence (logical, left-associative,
/// looser than the comparisons), its lowering, and its short-circuiting native
/// leaf. Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        1.2,
        Assoc::Left,
        Schedule::Infix,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("and", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// Build `lhs and rhs` as `{ty: and, value: [lhs, rhs, and_native]}`, requiring
/// both operands to be `bool` ([`ParseError::NonBoolOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    and: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    if !unsafe { is_bool_result(types, lhs) && is_bool_result(types, rhs) } {
        return Err(ParseError::NonBoolOperands);
    }
    // Two bool literals fold now (a bare literal is pure, so nothing is lost),
    // like `==` over rationals or types — what keeps a comptime chain comptime.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    if let (Some(a), Some(b)) =
        unsafe { (bool_literal_value(types, lhs), bool_literal_value(types, rhs)) }
    {
        return Ok(bool_mod::literal_node(store, types.bool_, a && b));
    }
    let value = store.alloc_operands(&[lhs, rhs, types.ops.and_]);
    Ok(store.alloc_raw(and, value))
}

/// Run: short-circuit — the result is `false` (without running the right operand)
/// when the left is false, else the right operand's value.
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

/// Lower: a short-circuiting two-way branch (`if a then b else false`).
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `and` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_and(lhs, rhs)
    }
}
