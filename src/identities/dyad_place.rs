// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What a `dyad ?` place holds: a value that is no node is written into a new node `[T | bytes]`
//! on the heap, which the place owns and frees when a write replaces it and when its life ends;
//! a node the store holds is held as it is and never freed. ›A `dyad ?` place is transparent: a
//! placeholder for a new node of any type‹ [[R293]].

use cranelift_codegen::ir::Value;

use super::assign::{load, lower_take, put, take};
use super::callable::{self, Callables};
use super::drop_model::{heap_alloc, heap_free, teardown_of, Teardown};
use super::read::{carry_of, place_layout, Carry, Read};
use super::{meta, numtype, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::{self, DyadPtr, PAYLOAD_OFFSET};
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct DyadPlaceIds {
    /// `[value, from, op, output_type]`: a new node of type `from` holding `value`; with `from`
    /// `dyad`, a copy of the node another `dyad ?` place holds, where that place owns it.
    pub new_node: DyadPtr,
    pub new_node_leaf: DyadPtr,
}

/// No spelling: the landing in a `dyad ?` place builds it.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DyadPlaceIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "from", "op", "output_type"],
    );
    let new_node = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(new_node, lower_new_node);
    let new_node_leaf = callable::mint_native(cx.store, cs.callable, run_new_node, cs.seed_native);
    DyadPlaceIds { new_node, new_node_leaf }
}

/// `value` landed in a `dyad ?` place: any node the store holds as it is; where the place's
/// holder ends what it takes (`held`), a value whose type moves whole in a new node of that type,
/// and what another `dyad ?` place holds copied, unless it moves out of where it was made.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn land(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    held: bool,
) -> Result<DyadPtr, ParseError> {
    if types.is_hole(types.through(value)) {
        return Ok(value);
    }
    let from = super::read::output_type(types, value);
    if from == types.dyad_ {
        let made = teardown_of(types, value) != Teardown::Nothing;
        return match (held, made) {
            (true, true) => Ok(value),
            (true, false) => Ok(build_new_node(store, types, value, from)),
            // A place no name holds borrows what another place holds, as a `static` function's
            // receiver does: stand-in for #256.
            (false, false) => Ok(value),
            (false, true) => Err(ParseError::DyadTakesNode),
        };
    }
    if held && is_data(types, from) {
        // The new node would have to run the value's own end: stand-in for #279.
        if has_end(types, from) {
            return Err(ParseError::DyadTakesNode);
        }
        return Ok(build_new_node(store, types, value, from));
    }
    if super::assign::box_takes(types, types.dyad_, value) {
        return Ok(value);
    }
    Err(ParseError::DyadTakesNode)
}

/// A type whose value lies in its place's bytes, a number, a pointer, a rational or a record,
/// and moves whole.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
unsafe fn is_data(types: &Core, t: DyadPtr) -> bool {
    matches!(
        place_layout(types, t),
        Some((Read::Scalar(_) | Read::Pointer(_) | Read::Rational | Read::Aggregate, _))
    ) && carry_of(types, t).is_some()
}

/// Whether a value of `t` ends by more than its bytes: an owning pointer, or a record whose
/// body fills `free`.
///
/// # Safety
/// `t` must be a type whose `place_layout` is a data read.
unsafe fn has_end(types: &Core, t: DyadPtr) -> bool {
    let t = super::type_identity_of(types, t).unwrap_or(t);
    if numtype::is_pointer_type(t) {
        return !meta::destructor_of(t).is_null();
    }
    meta::is_record_type(t) && !meta::instances_free_of(t).is_null()
}

pub(crate) fn build_new_node(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    from: DyadPtr,
) -> DyadPtr {
    store.alloc_words(
        types.dyad_place.new_node,
        &[value, from, types.dyad_place.new_node_leaf, types.dyad_],
    )
}

/// # Safety
/// `node` must be a node from [`build_new_node`].
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// Whether a `dyad ?` place made `held`: a node the store does not hold.
pub(crate) fn made(store: &Store, held: DyadPtr) -> bool {
    !held.is_null() && !store.holds(held as *const u8)
}

/// What a line the parse builds over `held`, the node a `dyad ?` place holds now, stands on: a
/// node the store holds as it is, and a copy the store holds of one the place made, so no line
/// outlives the node it reads.
///
/// # Safety
/// `held` must be a live node a `dyad ?` place holds.
pub(crate) unsafe fn stable(store: &mut Store, types: &Core, held: DyadPtr) -> DyadPtr {
    if !made(store, held) {
        return held;
    }
    let ty = dyad::ty(held);
    let Some(carry) = carry_of(types, ty) else {
        return held;
    };
    let bytes = std::slice::from_raw_parts(dyad::value(held) as *const u8, carry.width());
    store.alloc_blob(ty, bytes)
}

fn run_new_node(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build_new_node`; a `dyad` value is a node's address or 0.
    unsafe {
        let (value, from) = parts(node);
        if from == rt.types().dyad_ {
            let held = rt.run(value)? as usize as DyadPtr;
            return copy(rt, held);
        }
        let carry = carry_of(rt.types(), from).ok_or(RunError::NoWholeRead)?;
        let taken = take(rt, carry, value)?;
        make(rt, from, carry, taken)
    }
}

/// A new node of `ty` on the heap holding what [`take`] handed over.
///
/// # Safety
/// `taken` must be what `take` gave for `carry`, the carry of `ty`.
unsafe fn make(rt: &mut Runtime, ty: DyadPtr, carry: Carry, taken: i64) -> Result<i64, RunError> {
    let node = heap_alloc(PAYLOAD_OFFSET as usize + carry.width()) as DyadPtr;
    if node.is_null() {
        return Err(RunError::OutOfMemory);
    }
    dyad::set_ty(node, ty);
    put(carry, dyad::value(node), taken);
    rt.note_alloc();
    Ok(node as i64)
}

/// `held` again for a place of its own: a node the store holds as it is, one a place made
/// copied.
///
/// # Safety
/// `held` must be null or a live node.
unsafe fn copy(rt: &mut Runtime, held: DyadPtr) -> Result<i64, RunError> {
    if !made(rt.store, held) {
        return Ok(held as i64);
    }
    let ty = dyad::ty(held);
    let carry = carry_of(rt.types(), ty).ok_or(RunError::NoWholeRead)?;
    let taken = load(carry, dyad::value(held));
    make(rt, ty, carry, taken)
}

/// The end of what a `dyad ?` place held: the node it made is freed, a node the store holds
/// is not.
///
/// # Safety
/// `word` must be 0 or the address of a live node a `dyad ?` place held.
pub(crate) unsafe fn end(rt: &mut Runtime, word: i64) {
    let held = word as usize as DyadPtr;
    if made(rt.store, held) {
        heap_free(held as *mut u8);
        rt.note_free();
    }
}

fn lower_new_node(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as `run_new_node`.
    unsafe {
        let (value, from) = parts(node);
        if from == lw.types().dyad_ {
            let held = lw.lower(value)?;
            return lw.call_seed(compiled_copy as *const () as usize, &[held]);
        }
        let carry = carry_of(lw.types(), from).ok_or(CompileError::NotLowerable(node))?;
        let mut taken = lower_take(lw, carry, value)?;
        if let Carry::Word(nt) = carry {
            taken = lw.widen(taken, nt);
        }
        let ty = lw.node_addr(from);
        lw.call_seed(compiled_make as *const () as usize, &[ty, taken])
    }
}

/// # Safety
/// Called only by compiled code, with its context, `ty` a type whose value moves whole and
/// `taken` what `take` gives for it.
unsafe extern "C" fn compiled_make(ctx: *mut crate::run::Context, ty: DyadPtr, taken: i64) -> i64 {
    // SAFETY: the caller's contract.
    crate::run::interpret_step(ctx, |rt| unsafe {
        let carry = carry_of(rt.types(), ty).ok_or(RunError::NoWholeRead)?;
        make(rt, ty, carry, taken)
    })
}

/// # Safety
/// Called only by compiled code, with its context and `held` a node a `dyad ?` place holds.
unsafe extern "C" fn compiled_copy(ctx: *mut crate::run::Context, held: DyadPtr) -> i64 {
    // SAFETY: the caller's contract.
    crate::run::interpret_step(ctx, |rt| unsafe { copy(rt, held) })
}

/// # Safety
/// Called only by compiled code, with its context and `word` what a `dyad ?` place held.
pub(crate) unsafe extern "C" fn compiled_end(ctx: *mut crate::run::Context, word: i64) -> i64 {
    // SAFETY: the caller's contract.
    crate::run::interpret_step(ctx, |rt| unsafe {
        end(rt, word);
        Ok(0)
    })
}
