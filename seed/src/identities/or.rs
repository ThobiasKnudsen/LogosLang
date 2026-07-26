// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `or`: short-circuiting logical disjunction over `bool`s. Both operands must be
//! `bool` (comparisons, `bool` values, or nested logical results); the result is a
//! `bool`. It short-circuits: when the left operand is true the right is not
//! evaluated, so run branches and compile lowers a two-way branch (`if a then true
//! else b`, [`crate::compile::Lowerer::lower_or`]). It binds loosest of the logical
//! operators (looser than `and`), just above `=`. One concrete native serves it:
//! the node is `{logos: or, value: [lhs, rhs, or_native]}` and run jumps through the
//! op slot (issue #44).

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{bool_mod, meta, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{
    bool_literal_value, is_bool_result, Assoc, CoreTypes, ParseError,
};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `or`: spelling and parse precedence (logical, left-associative,
/// looser than `and`), its lowering, and its short-circuiting native leaf.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (SynolonPtr, SynolonPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        1.1,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("or", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

/// Build `lhs or rhs` as `{logos: or, value: [lhs, rhs, or_native]}`, requiring both
/// operands to be `bool` ([`ParseError::NonBoolOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    or: SynolonPtr,
    lhs: SynolonPtr,
    rhs: SynolonPtr,
) -> Result<SynolonPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced synolons from the store; reading their logos is safe.
    if !unsafe { is_bool_result(types, lhs) && is_bool_result(types, rhs) } {
        return Err(ParseError::NonBoolOperands);
    }
    // Two bool literals fold now (a bare literal is pure, so nothing is lost),
    // like `==` over rationals or logos — what keeps a comptime chain comptime.
    // SAFETY: `lhs`/`rhs` are reduced synolons from the store.
    if let (Some(a), Some(b)) =
        unsafe { (bool_literal_value(types, lhs), bool_literal_value(types, rhs)) }
    {
        return Ok(bool_mod::literal_node(store, types.bool_, a || b));
    }
    let value = store.alloc_operands(&[lhs, rhs, types.ops.or_]);
    Ok(store.alloc_raw(or, value))
}

/// Run: short-circuit — the result is `true` (without running the right operand)
/// when the left is true, else the right operand's value.
fn run(rt: &mut Runtime, node: SynolonPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        if rt.run(lhs)? != 0 {
            Ok(1)
        } else {
            rt.run(rhs)
        }
    }
}

/// Lower: a short-circuiting two-way branch (`if a then true else b`).
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_or(lhs, rhs)
    }
}
