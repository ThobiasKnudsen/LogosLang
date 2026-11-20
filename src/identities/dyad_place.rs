// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What a `dyad ?` place holds: a value that is no node is written into a new node `[T | bytes]`
//! on the heap, which the place owns and frees when a write replaces it and when its life ends;
//! a node the store holds is held as it is and never freed; and the one read of the node a
//! `dyad` value no name holds gives, wherever a value is read from it. ›A `dyad ?` place is
//! transparent: a placeholder for a new node of any type‹ [[R293]].

use cranelift_codegen::ir::Value;

use super::assign::{handed_type, load, lower_load, lower_take, put, take};
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
    /// `[value, into, ends, lossy, op, output_type]`: the value of type `into` read out of the
    /// node `value` gives; `ends` is non-null where `value` is made there, and the read ends that
    /// node unless its line's end does; `lossy` is non-null where the node lands in `into` as in
    /// `into lossy (…)`.
    pub read: DyadPtr,
    pub read_leaf: DyadPtr,
}

/// No spelling: the landing in a `dyad ?` place builds the new node, a read of a `dyad` value
/// the read.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DyadPlaceIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "from", "op", "output_type"],
    );
    let new_node = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `new_node` carries the record built above.
    unsafe { meta::mark_own(new_node) };
    cx.lower.insert(new_node, lower_new_node);
    let new_node_leaf = callable::mint_native(cx.store, cs.callable, run_new_node, cs.seed_native);
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "into", "ends", "lossy", "op", "output_type"],
    );
    let read = cx.store.alloc_head(cx.type_, record);
    cx.lower.insert(read, lower_read);
    let read_leaf = callable::mint_native(cx.store, cs.callable, run_read, cs.seed_native);
    DyadPlaceIds { new_node, new_node_leaf, read, read_leaf }
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

/// `dyad (ty, value)`: `value` landed in `ty` where the line is written, held in a new node of
/// `ty` each time the line runs (›`dyad (T, v)` makes a new node of `T` holding `v` each time its
/// line runs‹ [[R371]]).
///
/// # Safety
/// `ty` must be a type identity and `value` a reduced dyad, both from the store.
pub(crate) unsafe fn build_of_type(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    // A value with an end of its own would need the new node to run it: stand-in for #279. A
    // value that is a node itself: stand-in for #295.
    if ty == types.dyad_ || !is_data(types, ty) || has_end(types, ty) {
        return Err(ParseError::BadDyadType);
    }
    let landed = super::land(store, types, value, ty)?;
    Ok(build_new_node(store, types, landed, ty))
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

/// Whether a value of `t` moves whole into a new node of its type and ends with its bytes.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
pub(crate) unsafe fn moves_whole(types: &Core, t: DyadPtr) -> bool {
    is_data(types, t) && !has_end(types, t)
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

/// The type of the node `value` makes, where that is known where it is written: `dyad (T, v)`'s
/// `T`, which a `.type` on it reads through to (›A `dyad ?` place is transparent: a placeholder
/// for a new node of any type‹ [[R293]]).
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn made_type(types: &Core, value: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(value);
    if dyad::ty(node) != types.dyad_place.new_node {
        return None;
    }
    let (_, from) = parts(node);
    (from != types.dyad_).then_some(from)
}

/// Whether the node `value` gives, and so its type, is known only when the program runs: a
/// `dyad` place or call the pass has not read through. A hole gives no node, and `dyad (T, v)`
/// gives one of `T`.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn known_only_at_run(types: &Core, value: DyadPtr) -> bool {
    super::read::output_type(types, value) == types.dyad_
        && !types.is_hole(types.through(value))
        && made_type(types, value).is_none()
}

/// What `value` gives where a value is read from it, when it gives a `dyad` no name holds: the
/// value the node it gives holds (›A `dyad ?` place is transparent: a placeholder for a new
/// node of any type‹ [[R293]]), read each time the line runs at the type that node is made of
/// where that is known where `value` is written (`dyad (T, v)`'s `T`), else at `into`, the type
/// of the place it lands in, which the node lands in when it runs, as `into lossy (…)` with
/// `lossy`. `None` for any other value, into a `dyad ?` or `type ?` box, which takes the node,
/// and where neither type is known here.
///
/// # Safety
/// `value` must be a reduced dyad from the store; `into` null or a type node.
pub(crate) unsafe fn read_through(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    into: DyadPtr,
    lossy: bool,
) -> Option<DyadPtr> {
    if super::read::output_type(types, value) != types.dyad_ || types.is_hole(types.through(value))
    {
        return None;
    }
    let boxed = |t: DyadPtr| t == types.dyad_ || t == types.type_;
    if !into.is_null()
        && matches!(place_layout(types, into), Some((Read::Container(t), _)) if boxed(t))
    {
        return None;
    }
    let read_as = made_type(types, value).or((!into.is_null()).then_some(into))?;
    let mark = |set: bool, word: DyadPtr| if set { word } else { std::ptr::null_mut() };
    let ends = mark(teardown_of(types, value) == Teardown::Dyad, types.dyad_);
    let lossy = mark(lossy, types.lossy);
    let ids = types.dyad_place;
    Some(store.alloc_words(ids.read, &[value, read_as, ends, lossy, ids.read_leaf, read_as]))
}

/// # Safety
/// `node` must be a node from [`build_new_node`] or [`read_through`].
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// The read `node`'s two marks: whether it ends the node its value gives, one made there that no
/// line's end ends, and whether that node lands `lossy`.
///
/// # Safety
/// `node` must be a node from [`read_through`].
unsafe fn read_marks(node: DyadPtr) -> (bool, bool) {
    let p = dyad::value(node) as *const DyadPtr;
    (!(*p.add(2)).is_null(), !(*p.add(3)).is_null())
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

fn run_read(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `read_through`; a `dyad` value is a node's address or 0.
    unsafe {
        let (value, into) = parts(node);
        let (ends, lossy) = read_marks(node);
        let held = rt.run(value)? as usize as DyadPtr;
        if held.is_null() {
            return Err(RunError::NullPointer);
        }
        let read = read_held(rt, held, into, lossy);
        if ends && !rt.planned(value) {
            end(rt, held as i64);
        }
        read
    }
}

/// The value of `into` `held` gives: its own where it is of `into`, else what it lands as there,
/// refused as the parse error it is where it cannot; one handed on by its bytes' address is
/// copied into the line's scratch, so it outlives `held`, which may end first.
///
/// # Safety
/// `held` must be a live node; `into` a type node.
unsafe fn read_held(
    rt: &mut Runtime,
    held: DyadPtr,
    into: DyadPtr,
    lossy: bool,
) -> Result<i64, RunError> {
    let types: *const Core = rt.types();
    let carry = carry_of(&*types, into);
    let value = match carry {
        Some(carry) if dyad::ty(held) == into && is_data(&*types, into) => {
            load(carry, dyad::value(held))
        }
        _ => {
            let node = stable(rt.store(), &*types, held);
            let landed = super::land_as(rt.store(), &*types, node, into, lossy)
                .map_err(|e| RunError::Parse(Box::new(e)))?;
            match carry {
                Some(bytes @ Carry::Bytes(_)) => take(rt, bytes, landed)?,
                _ => rt.run(landed)?,
            }
        }
    };
    let Some(Carry::Bytes(width)) = carry else {
        return Ok(value);
    };
    let copy = rt.scratch(width);
    std::ptr::copy_nonoverlapping(value as usize as *const u8, copy, width);
    Ok(copy as i64)
}

/// Only where the node's type is known where the value is written: a read whose node lands at
/// run stays interpreted.
fn lower_read(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as `run_read`.
    unsafe {
        let (value, into) = parts(node);
        if made_type(lw.types(), value) != Some(into) {
            return Err(CompileError::NotLowerable(node));
        }
        let carry = carry_of(lw.types(), into).ok_or(CompileError::NotLowerable(node))?;
        let ends = read_marks(node).0 && !lw.planned(value);
        let held = lw.lower(value)?;
        lw.guard_non_null(held, handed_type(carry), |lw| {
            let mut read = lower_load(lw, carry, held, PAYLOAD_OFFSET);
            if let Carry::Bytes(width) = carry {
                let copy = lw.scratch_slot(width);
                lw.copy_bytes(copy, read, width);
                read = copy;
            }
            if ends {
                lw.call_seed(compiled_end as *const () as usize, &[held])?;
            }
            Ok(read)
        })
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
