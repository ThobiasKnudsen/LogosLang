// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `-`: subtraction. Like `+` (see [`crate::identities::plus`]), a parse-time
//! constructor owning no code: it resolves each application to a concrete
//! subtraction and stores the leaf in the op slot
//! `{type: -, value: [lhs, rhs, sub_<logos>]}`. Same precedence as `+`,
//! left-associative.

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `-`: spelling, precedence (same as `+`, left-associative), and its
/// lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::ADDITIVE,
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
/// unsigned; a negative literal is folded at discovery by the literal's own
/// constructor); anything else to the right is negated as `0 - x`.
fn construct(
    p: &mut crate::parse::Parser,
    id: DyadPtr,
    tape: &mut crate::parse::ParsingTape,
) -> Result<crate::parse::Constructed, ParseError> {
    if let Some((lhs, rhs)) = p.binary_operands(tape)? {
        let types = p.types();
        let node = build(p.store(), &types, id, lhs, rhs)?;
        tape.reduce_here(node);
        return Ok(crate::parse::Constructed::Placed);
    }
    // Prefix: negation of the operand to the right (DESIGN ›Numeric
    // literals‹, ruled 5 September 2026: one identity whose constructor reads
    // its left context) — spelled `0 - x`, the sketch's own spelling of a
    // negative, so it molds to the operand's logos and lowers as a
    // subtraction. A literal to the right was already folded into a negative
    // literal at discovery, so this is the non-literal case.
    let types = p.types();
    let rhs = p.take_right(tape)?;
    let zero = rational::build(p.store(), types.rational, "0")?;
    let node = build(p.store(), &types, id, zero, rhs)?;
    tape.place(node);
    Ok(crate::parse::Constructed::Placed)
}

/// Build `lhs - rhs`: resolve the operand logos and store the concrete
/// subtraction in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    minus: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // Two comptime rationals fold now (exact fraction math); otherwise resolve and build.
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Sub, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Sub, nt)]);
    Ok(store.alloc_raw(minus, value))
}

/// Lower: emit the machine subtraction for the resolved operand logos.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `-` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Sub) }
}
