// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `==`: equality. Like `<` (see [`crate::identities::lt`]), a parse-time
//! constructor resolving each application to a concrete comparison in the op
//! slot; result is `bool`. Equality binds looser than the relational operators;
//! the trie longest-matches `==` over `=`.

use crate::Core;
use cranelift_codegen::ir::Value;

use super::numtype::CmpOp;
use super::numtype::NumType;
use super::read::Read;
use super::{bool_mod, meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::store::Store;

/// Register `==`: spelling, parse_rank (equality, left-associative), and its
/// lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::EQUALITY,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("==", id);
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs == rhs`: resolve the operand logos and store the concrete
/// comparison in the op slot.
fn build(
    store: &mut Store,
    types: &Core,
    eq: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now to a `bool` literal; otherwise resolve and build.
    if let Some(v) = rational::compare_literals(types, CmpOp::Eq, lhs, rhs) {
        return Ok(bool_mod::literal_node(store, types.bool_, v));
    }
    // What the two operands are is the reading rule's answer (#82). Two
    // identities in hand compare by identity now: logos are interned, so
    // pointer identity *is* logos identity (roadmap #30) — `x:dyad.type == i32`
    // is decided at parse. Two values that read as node addresses — an
    // identity against a box or a view, two boxes, a bare parameter that
    // "accepts any dyad" — compare those addresses when the program runs
    // (DESIGN ›A type is a comptime value‹, 12 September 2026: a type value
    // "may be passed to a function, held in a place, and compared").
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let (l, r) =
        unsafe { (super::read::read_kind(types, lhs), super::read::read_kind(types, rhs)) };
    if l == Read::Identity && r == Read::Identity {
        // SAFETY: `lhs`/`rhs` are reduced dyads from the store (above).
        let same = unsafe { types.through(lhs) == types.through(rhs) };
        return Ok(bool_mod::literal_node(store, types.bool_, same));
    }
    let addressed = |k: Read| matches!(k, Read::Identity | Read::Container(_) | Read::Address);
    if addressed(l) && addressed(r) {
        let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(CmpOp::Eq, NumType::I64)]);
        return Ok(store.alloc_raw(eq, value));
    }
    // Two addresses compare as addresses (DESIGN ›Declarations are immutable
    // by default‹: "`&x` and `&y` differ"; #123: which of a function's scopes
    // one stands in is found "comparing its scopes' addresses with the scope
    // at hand"). Pointer arithmetic stays refused below.
    // SAFETY: as above.
    let pointer =
        |n: DyadPtr| unsafe { matches!(super::numtype_of(types, n), super::Operand::Pointer(_)) };
    if pointer(lhs) && pointer(rhs) {
        let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(CmpOp::Eq, NumType::I64)]);
        return Ok(store.alloc_raw(eq, value));
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.cmp_leaf(CmpOp::Eq, nt)]);
    Ok(store.alloc_raw(eq, value))
}

/// Lower: emit the machine comparison for the resolved operand logos.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `==` application `[lhs, rhs, op]`.
    unsafe { lw.lower_compare(node, CmpOp::Eq) }
}
