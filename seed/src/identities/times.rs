// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `*`: multiplication. Like `+` (see [`crate::identities::plus`]), a
//! parse-time constructor owning no code: it resolves each application to a
//! concrete multiplication and stores the leaf in the op slot
//! `{ty: *, value: [lhs, rhs, mul_<type>]}`. Binds tighter than `+`/`-`,
//! left-associative.

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `*`: spelling, precedence (binding tighter than `+`/`-`,
/// left-associative), and its lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        3.0,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("*", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs * rhs`: resolve the operand type and store the concrete
/// multiplication in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    times: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math); otherwise resolve and build.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Mul, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Mul, nt)]);
    Ok(store.alloc_raw(times, value))
}

/// Lower: emit the machine multiplication for the resolved operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `*` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Mul) }
}
