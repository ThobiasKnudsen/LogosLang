// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `+`: addition. `+` is a *higher-level identity* describing how to read its
//! operands (DESIGN ›which concrete machine operation runs is resolved from the
//! operand types‹): its node is `{ty: +, value: [lhs, rhs, type]}`, where the
//! resolved operand type is stored in the value slot. Run and compile read that
//! stored type and switch on it (see [`crate::identities::numtype`]), so one `+`
//! identity serves every numeric type.

use cranelift_codegen::ir::Value;

use super::numtype::{eval_arith, ArithOp};
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// Register `+`: spelling, parse precedence (left-associative, binding tighter than
/// `=`), and its type-switched run and lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record =
        meta::operand_record(cx, meta::TUPLE_TAG, 2.0, Assoc::Left, &["lhs", "rhs", "type"]);
    let id = cx.store.alloc_raw(cx.fn_type, record);
    cx.trie.insert("+", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.bcode.insert(id, run);
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs + rhs`: resolve the operand type and store it as the node's third
/// operand, giving `{ty: +, value: [lhs, rhs, type]}`. Resolution follows
/// [`resolve_binary`]: matching concrete types keep theirs, a literal molds to its
/// partner, two literals fold exactly; non-numeric operands leave `+` unresolved
/// ([`ParseError::UnsupportedOperands`]).
fn build(
    store: &mut Store,
    types: &CoreTypes,
    plus: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math), staying rational until
    // context types them; otherwise resolve the operand type and build the `+` node.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Add, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ops = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&ops);
    Ok(store.alloc_raw(plus, value))
}

/// Run: add in the stored operand type.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a valid `+` application `[lhs, rhs, type]`.
    unsafe { eval_arith(rt, node, ArithOp::Add) }
}

/// Lower: emit the machine addition for the stored operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `+` application `[lhs, rhs, type]`.
    unsafe { lw.lower_arith(node, ArithOp::Add) }
}
