// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `or`: short-circuiting logical disjunction over `bool`s. Both operands must be
//! `bool` (comparisons, `bool` values, or nested logical results); the result is a
//! `bool`. It short-circuits: when the left operand is true the right is not
//! evaluated, so run branches and compile lowers a two-way branch (`if a then true
//! else b`, [`crate::compile::Lowerer::lower_or`]). It binds loosest of the logical
//! operators (looser than `and`), just above `=`. One concrete native serves it:
//! the node is `{ty: or, value: [lhs, rhs, or_native]}` and run jumps through the
//! op slot (issue #44).

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{meta, operands, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{is_bool_result, Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `or`: spelling and parse precedence (logical, left-associative,
/// looser than `and`), its lowering, and its short-circuiting native leaf.
/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, 1.1, Assoc::Left, &["lhs", "rhs", "op"]);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("or", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.lower.insert(id, lower);
    let leaf = callable::mint(cx.store, cs.callable, run as usize, cs.seed_native);
    (id, leaf)
}

/// Build `lhs or rhs` as `{ty: or, value: [lhs, rhs, or_native]}`, requiring both
/// operands to be `bool` ([`ParseError::NonBoolOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    or: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store; reading their type is safe.
    if !unsafe { is_bool_result(types, lhs) && is_bool_result(types, rhs) } {
        return Err(ParseError::NonBoolOperands);
    }
    let value = store.alloc_operands(&[lhs, rhs, types.ops.or_]);
    Ok(store.alloc_raw(or, value))
}

/// Run: short-circuit — the result is `true` (without running the right operand)
/// when the left is true, else the right operand's value.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
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
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `or` application; its two operands are valid.
    unsafe {
        let (lhs, rhs) = operands(node);
        lw.lower_or(lhs, rhs)
    }
}
