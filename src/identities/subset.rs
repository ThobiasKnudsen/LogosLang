// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `⊆`: inclusion between types, `A ⊆ B` true when every value of `A` is a value of `B`
//! (DESIGN ›Inclusion between types is `⊆`‹). Two types known at parse fold to a `bool`; a type
//! read at run, such as `tape[1]:type`, builds `[lhs, rhs, op]`, run interpreted.

use super::callable::{self, Callables};
use super::read::Read;
use super::{bool_mod, meta, Cx, NumType};
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// Returns `(identity, leaf)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr) {
    // `==`'s rank, above `not`, as a stand-in for #137: DESIGN gives `⊆` no rank.
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::EQUALITY,
        Assoc::Left,
        &["lhs", "rhs", "op"],
    );
    let id = cx.store.alloc_raw(cx.type_, record);
    cx.declare("⊆", id);
    cx.metas.insert(id, super::infix_construct!(build));
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
    (id, leaf)
}

fn build(
    store: &mut Store,
    types: &Core,
    op: DyadPtr,
    lhs: DyadPtr,
    rhs: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // SAFETY: `lhs`/`rhs` are reduced dyads from the store.
    unsafe {
        let (l, r) = (super::read::read_kind(types, lhs), super::read::read_kind(types, rhs));
        if l == Read::Identity && r == Read::Identity {
            let truth = includes(types, types.through(lhs), types.through(rhs))
                .ok_or(ParseError::UnsettledInclusion)?;
            return Ok(bool_mod::literal_node(store, types.bool_, truth));
        }
        if type_valued(types, lhs, l) && type_valued(types, rhs, r) {
            let value = store.alloc_operands(&[lhs, rhs, types.ops.subset_]);
            return Ok(store.alloc_raw(op, value));
        }
    }
    Err(ParseError::UnsupportedOperands)
}

/// A type standing as a value, a `type` box, or a tape cell's `:type`: each reads as a
/// type's address.
///
/// # Safety
/// `n` is a reduced dyad from the store.
unsafe fn type_valued(types: &Core, n: DyadPtr, k: Read) -> bool {
    matches!(k, Read::Identity)
        || k == Read::Container(types.type_)
        || (*types.through(n)).ty == types.tape.cell_type
}

/// `None` where DESIGN leaves the answer open: two different types unless both are integers.
///
/// # Safety
/// `a` and `b` are null or dyads from the store.
unsafe fn includes(types: &Core, a: DyadPtr, b: DyadPtr) -> Option<bool> {
    let is_type = |t: DyadPtr| !t.is_null() && (*t).ty == types.type_;
    if !is_type(a) || !is_type(b) {
        return None;
    }
    if a == b {
        return Some(true);
    }
    let integer = |t: DyadPtr| {
        let nt = types.numtypes.iter().position(|&n| n == t)? as u8;
        let nt = NumType::from_tag(nt);
        (!nt.is_float()).then_some(nt)
    };
    let (a, b) = (integer(a)?, integer(b)?);
    // An unsigned type needs a strictly wider signed one to hold its top half.
    Some(match (a.is_signed_int(), b.is_signed_int()) {
        (true, false) => false,
        (false, true) => a.bytes() < b.bytes(),
        _ => a.bytes() <= b.bytes(),
    })
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `⊆` node `[lhs, rhs, op]`; each operand runs to a type's address.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let a = rt.run(*ops)? as DyadPtr;
        let b = rt.run(*ops.add(1))? as DyadPtr;
        includes(rt.types(), a, b).map(i64::from).ok_or(RunError::UnsettledInclusion)
    }
}
