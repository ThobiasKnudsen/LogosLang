// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `%`: remainder. Like `*` it stores its resolved operand type in
//! `{ty: %, value: [lhs, rhs, type]}` and run/compile switch on it; binds like
//! `*`, left-associative. Integer remainder is TOTAL (settled): `x % 0` yields
//! the type's MAX — the same loud sentinel as `/` — and a signed `x % -1` is the
//! well-defined 0. Float `%` is rejected at parse (Cranelift has no float
//! remainder instruction; a libcall path can lift this later). Two comptime
//! *integer* literals fold exactly; a fractional literal falls through to the
//! committed runtime path, and a literal zero divisor is a parse error.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_arith, of_type_node, ArithOp};
use super::{rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `%`: spelling, precedence (binding like `*`, left-associative), and
/// its type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let id = cx.store.alloc_raw(cx.fn_type, std::ptr::null_mut());
    cx.trie.insert("%", IdContext::new(id, cx.root_scope));
    cx.metas
        .insert(id, Construct::Infix { precedence: 3.0, assoc: Assoc::Left, build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs % rhs`: fold two comptime integer literals exactly, else resolve
/// the operand type — rejecting floats, which have no machine remainder — and
/// store it as the third operand.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    rem: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Rem, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ops = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    // SAFETY: `ops[2]` is the resolved numeric type node.
    if unsafe { of_type_node(ops[2]) }.is_float() {
        return Err(ParseError::UnsupportedOperands);
    }
    let value = store.alloc_operands(&ops);
    Ok(store.alloc_raw(rem, value))
}

/// Run: take the remainder in the stored operand type (total semantics; see
/// [`ArithOp`]).
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `%` application `[lhs, rhs, type]`.
    unsafe { eval_arith(rt, node, ArithOp::Rem) }
}

/// Lower: emit the checked machine remainder for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `%` application `[lhs, rhs, type]`.
    unsafe { lw.lower_arith(node, ArithOp::Rem) }
}
