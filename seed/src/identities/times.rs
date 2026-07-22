// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `*`: multiplication. Like `+` (see [`crate::identities::plus`]), a
//! parse-time constructor owning no code: it resolves each application to a
//! concrete multiplication and stores the leaf in the op slot
//! `{logos: *, value: [lhs, rhs, mul_<logos>]}`. Binds tighter than `+`/`-`,
//! left-associative.

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `*`: spelling, precedence (binding tighter than `+`/`-`,
/// left-associative), and its lowering.
pub(super) fn register(cx: &mut Cx) -> SynolonPtr {
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

/// Build `lhs * rhs`: resolve the operand logos and store the concrete
/// multiplication in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    times: SynolonPtr,
    lhs: SynolonPtr,
    rhs: SynolonPtr,
) -> Result<SynolonPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math); otherwise resolve and build.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Mul, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced synolons from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Mul, nt)]);
    Ok(store.alloc_raw(times, value))
}

/// Lower: emit the machine multiplication for the resolved operand logos.
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `*` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Mul) }
}
