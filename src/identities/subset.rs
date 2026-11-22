// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `⊆`: inclusion between types, `A ⊆ B` true when every value of `A` is a value of `B`
//! (DESIGN ›Inclusion between types is `⊆`‹). Two types known at parse fold to a `bool`; a type
//! read at run, such as `tape[1].type`, builds `[lhs, rhs, op]`, run interpreted.

use super::callable::{self, Callables};
use super::numtype::is_pointer_type;
use super::read::Read;
use super::{bool_mod, meta, Cx, NumType};
use crate::dyad;
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
        &["lhs", "rhs", "op", "output_type"],
    );
    let id = cx.store.alloc_head(cx.type_, record);
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
            return Ok(store.alloc_words(op, &[lhs, rhs, types.ops.subset_, types.bool_]));
        }
    }
    Err(ParseError::UnsupportedOperands)
}

/// A type standing as a value, a `type` box, or the `.type` of a line a parse holds: each reads
/// as a type's address.
///
/// # Safety
/// `n` is a reduced dyad from the store.
unsafe fn type_valued(types: &Core, n: DyadPtr, k: Read) -> bool {
    matches!(k, Read::Identity)
        || k == Read::Container(types.type_)
        || super::tape::is_line_type_read(types, types.through(n))
}

/// Whether every value of `a` is a value of `b`: the one lies-inside test, which a conversion
/// written or not asks too (DESIGN ›A type's `parse` decides what converts into it; unwritten, a
/// value converts only into a type that holds it exactly‹); two pointer types by what a pointer
/// lands in. Each is read through its top gate node, `mut i32` as `i32`, since a gate changes
/// who reads and writes a value and not which values the type holds (›Gates are type nodes:
/// `x := mut i32 5`; the record holds no gates‹ [[R381]]). `None` where DESIGN leaves the answer
/// open: two different types that are not both pointer types, number types or `bool`, unless one
/// is a type a Logos `parse` builds, whose values are its own nodes alone.
///
/// # Safety
/// `a` and `b` are null or dyads from the store.
pub(crate) unsafe fn includes(types: &Core, a: DyadPtr, b: DyadPtr) -> Option<bool> {
    let (a, b) = (super::gate::under(a), super::gate::under(b));
    let is_type = |t: DyadPtr| !t.is_null() && dyad::ty(t) == types.type_;
    if !is_type(a) || !is_type(b) {
        return None;
    }
    if a == b {
        return Some(true);
    }
    if is_pointer_type(a) && is_pointer_type(b) {
        return Some(super::pointer::lands_in(a, b));
    }
    if super::meta::is_node_valued(a, types.fn_type)
        || super::meta::is_node_valued(b, types.fn_type)
    {
        return Some(false);
    }
    let number = |t: DyadPtr| {
        let nt = types.numtypes.iter().position(|&n| n == t)? as u8;
        Some(NumType::from_tag(nt))
    };
    if a == types.bool_ || b == types.bool_ {
        let other = if a == types.bool_ { b } else { a };
        return number(other).map(|_| false);
    }
    let (a, b) = (number(a)?, number(b)?);
    Some(if a.is_float() {
        b.is_float() && a.bytes() <= b.bytes()
    } else if a.is_signed_int() && !b.is_signed_int() && !b.is_float() {
        false
    } else {
        a.digits() <= b.digits()
    })
}

/// A line a parse holds that is a literal answers by whether the literal molds into the number
/// type, a literal taking the type beside it.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `⊆` node `[lhs, rhs, op]`; each operand runs to a type's address.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let read = rt.through(*ops);
        let b = rt.run(*ops.add(1))? as DyadPtr;
        let types = rt.types();
        if super::tape::is_line_type_read(types, read) && super::is_numtype_node(types, b) {
            let cell = super::tape::line_of_type_read(rt, read)?;
            if dyad::ty(cell) == rt.types().rational {
                let nt = super::numtype::of_type_node(b);
                return Ok(i64::from(super::rational::mold_to(cell, nt).is_some()));
            }
        }
        let a = rt.run(*ops)? as DyadPtr;
        includes(rt.types(), a, b).map(i64::from).ok_or(RunError::UnsettledInclusion)
    }
}
