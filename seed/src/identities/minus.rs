// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `-`: subtraction. Like `+` (see [`crate::identities::plus`]), a parse-time
//! constructor owning no code: it resolves each application to a concrete
//! subtraction and stores the leaf in the op slot
//! `{logos: -, value: [lhs, rhs, sub_<logos>]}`. Same precedence as `+`,
//! left-associative.

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `-`: spelling, precedence (same as `+`, left-associative), and its
/// lowering.
pub(super) fn register(cx: &mut Cx) -> SynolonPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        2.0,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("-", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, construct);
    cx.lower.insert(id, lower);
    id
}

/// `-`'s constructor. At reduction (two completed operands flanking the
/// cursor) it is ordinary subtraction. Opening fresh — no left operand — it
/// prefixes a numeric literal (`f(-1)`, `x := -5`; the literal regex is
/// unsigned, so the negative literal is this constructor negating at parse);
/// anything else declines, and the driver shifts the `-` as a pending operator
/// (general unary minus over non-literals is later work — it still parses as a
/// dangling operator today).
fn construct(
    p: &mut crate::parse::Parser,
    id: SynolonPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    if let Some((lhs, rhs)) = p.binary_operands(tape)? {
        let types = p.types();
        let node = build(p.store(), &types, id, lhs, rhs)?;
        tape.reduce_here(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    let types = p.types();
    match p.consume_rational()? {
        Some(lit) => {
            // SAFETY: `lit` is the rational literal just built.
            let neg = unsafe { rational::negate(p.store(), types.rational, lit) };
            tape.place(neg);
            Ok(crate::parse::Constructed::Placed)
        }
        None => Ok(crate::parse::Constructed::Decline),
    }
}

/// Build `lhs - rhs`: resolve the operand logos and store the concrete
/// subtraction in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    minus: SynolonPtr,
    lhs: SynolonPtr,
    rhs: SynolonPtr,
) -> Result<SynolonPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math); otherwise resolve and build.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Sub, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced synolons from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Sub, nt)]);
    Ok(store.alloc_raw(minus, value))
}

/// Lower: emit the machine subtraction for the resolved operand logos.
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `-` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Sub) }
}
