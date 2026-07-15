// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `+`: addition. `+` is a *parse-time constructor* owning no code (DESIGN ›The
//! callable ground is `@exec`‹; issue #44): from its operand types it resolves
//! each application to one concrete machine operation and stores that leaf in
//! the node's op slot — `{ty: +, value: [lhs, rhs, add_i32]}` — so run jumps
//! through the node and compile reads the same resolution. One `+` identity
//! serves every numeric type; the concrete additions are callable leaves
//! ([`crate::identities::ops`]).

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::store::Store;

/// Register `+`: spelling, parse precedence (left-associative, binding tighter
/// than `=`), and its lowering. A plain type — its record is parse/layout
/// metadata; the executable code lives on the leaves its applications reference.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, 2.0, Assoc::Left, &["lhs", "rhs", "op"]);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("+", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs + rhs`: resolve the operand type and store the concrete addition in
/// the op slot, giving `{ty: +, value: [lhs, rhs, add_<type>]}`. Resolution follows
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
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Add, nt)]);
    Ok(store.alloc_raw(plus, value))
}

/// Lower: emit the machine addition for the resolved operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `+` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Add) }
}
