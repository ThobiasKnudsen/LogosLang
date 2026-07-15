// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `%`: remainder. Like `*`, a parse-time constructor owning no code: it
//! resolves each application to a concrete remainder and stores the leaf in the
//! op slot `{ty: %, value: [lhs, rhs, rem_<type>]}`; binds like `*`,
//! left-associative. Integer remainder is TOTAL (settled): `x % 0` yields the
//! type's MAX — the same loud sentinel as `/` — and a signed `x % -1` is the
//! well-defined 0. Float `%` is rejected at parse (Cranelift has no float
//! remainder instruction, so no `rem_f32`/`rem_f64` leaf exists; a libcall path
//! can lift this later). Two comptime *integer* literals fold exactly; a
//! fractional literal falls through to the committed runtime path, and a
//! literal zero divisor is a parse error.

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, Construct, CoreTypes, ParseError};
use crate::store::Store;

/// Register `%`: spelling, precedence (binding like `*`, left-associative), and
/// its lowering.
pub(super) fn register(cx: &mut Cx) -> DyadPtr {
    let record = meta::operand_record(cx, meta::TUPLE_TAG, 3.0, Assoc::Left, &["lhs", "rhs", "op"]);
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("%", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, Construct::Infix { build });
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs % rhs`: fold two comptime integer literals exactly, else resolve
/// the operand type — rejecting floats, which have no machine remainder and
/// therefore no leaf — and store the concrete remainder in the op slot.
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
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    if nt.is_float() {
        return Err(ParseError::UnsupportedOperands);
    }
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Rem, nt)]);
    Ok(store.alloc_raw(rem, value))
}

/// Lower: emit the checked machine remainder for the resolved operand type.
fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `%` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Rem) }
}
