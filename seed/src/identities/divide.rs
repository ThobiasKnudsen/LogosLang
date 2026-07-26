// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `/`: division. Like `*` (see [`crate::identities::times`]), a parse-time
//! constructor owning no code: it resolves each application to a concrete
//! division and stores the leaf in the op slot
//! `{logos: /, value: [lhs, rhs, div_<logos>]}`; binds like `*`, left-associative.
//! Integer division truncates toward zero and is TOTAL (settled): a zero
//! divisor yields the logos's MAX — a loud sentinel, easier to discover than 0 —
//! and the signed MIN/-1 overflow saturates to MAX. Float division is IEEE
//! (`x / 0.0` is ±inf). Two comptime literals fold to an exact fraction —
//! `1 / 3` *is* one third — with a literal zero divisor a parse error; explicit
//! truncation is the cast (`i32(10 / 3)`).

use cranelift_codegen::ir::Value;

use super::numtype::ArithOp;
use super::{meta, rational, resolve_binary, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::synolon::SynolonPtr;
use crate::id_context::IdContext;
use crate::parse::{Assoc, CoreTypes, ParseError};
use crate::store::Store;

/// Register `/`: spelling, precedence (binding like `*`, left-associative), and
/// its lowering.
pub(super) fn register(cx: &mut Cx) -> SynolonPtr {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        3.0,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.trie.insert("/", IdContext::new(id, cx.root_scope));
    cx.metas.insert(id, super::infix_construct!(build));
    cx.lower.insert(id, lower);
    id
}

/// Build `lhs / rhs`: fold two comptime literals to an exact fraction, else
/// resolve the operand logos and store the concrete division in the op slot.
fn build(
    store: &mut Store,
    types: &CoreTypes,
    div: SynolonPtr,
    lhs: SynolonPtr,
    rhs: SynolonPtr,
) -> Result<SynolonPtr, ParseError> {
    if let Some(folded) = rational::fold_arith(store, types.rational, ArithOp::Div, lhs, rhs)? {
        return Ok(folded);
    }
    // SAFETY: `lhs`/`rhs` are reduced synolons from the store.
    let ([lhs, rhs], nt) = unsafe { resolve_binary(store, types, lhs, rhs) }?;
    let value = store.alloc_operands(&[lhs, rhs, types.ops.arith_leaf(ArithOp::Div, nt)]);
    Ok(store.alloc_raw(div, value))
}

/// Lower: emit the checked machine division for the resolved operand logos.
fn lower(lw: &mut Lowerer, node: SynolonPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a valid `/` application `[lhs, rhs, op]`.
    unsafe { lw.lower_arith(node, ArithOp::Div) }
}
