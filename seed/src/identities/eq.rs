// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `==`: equality. Like `<` (see [`crate::identities::lt`]), a parse-time
//! constructor resolving each application to a concrete comparison in the op
//! slot; result is `bool`. Equality binds looser than the relational operators;
//! the trie longest-matches `==` over `=`.

use cranelift_codegen::ir::Value;

use super::numtype::CmpOp;
use super::{bool_mod, is_type_value, meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::store::Store;

/// Register `==`: spelling, precedence (equality, left-associative), and its
/// lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, 1.4, Assoc::Left, &["lhs", "rhs", "op"]);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("==", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs == rhs`: resolve the operand type and store the concrete
/// comparison in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    eq: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now to a `bool` literal; otherwise resolve and build.
    if let Some(v) = rational::compare_literals(types.rational, CmpOp::Eq, lhs, rhs) {
        return Ok(bool_mod::literal_node(store, types.bool_, v));
    }
    // Two type-values compare by identity: types are interned, so pointer identity
    // *is* type identity and a type never varies at runtime, making the comparison a
    // parse-time constant (roadmap #30). This is what powers `x.type == i32`.
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    if unsafe { is_type_value(types, lhs) && is_type_value(types, rhs) } {
        return Ok(bool_mod::literal_node(store, types.bool_, lhs == rhs));
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(CmpOp::Eq, nt)]);
    Ok(store.alloc_raw(eq, value))
}

/// Lower: emit the machine comparison for the resolved operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `==` application `[lhs, rhs, op]`.
    unsafe { lw.lower_compare(node, CmpOp::Eq) }
}
