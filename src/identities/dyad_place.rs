// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! What a `dyad ?` place holds: a value that is no node is written into a new node `[T | bytes]`
//! on the heap, which the place owns and frees, with the value's own end unless it borrows the
//! value, when a write replaces it and when its life ends; a node the store holds is held as it is
//! and never freed; and the one read of the node a `dyad` value no name holds gives, wherever a
//! value is read from it. ›A `dyad ?` place is transparent: a placeholder for a new node of any
//! type‹ [[R293]].

use cranelift_codegen::ir::Value;

use super::assign::{handed_type, load, lower_load, lower_take, put, take};
use super::callable::{self, Callables};
use super::drop_model::{
    end_of_type, heap_alloc, heap_free, teardown_of, teardown_of_type, Teardown,
};
use super::read::{carry_of, place_layout, Carry, Read};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::{self, DyadPtr, PAYLOAD_OFFSET};
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

#[derive(Debug, Clone, Copy)]
pub struct DyadPlaceIds {
    /// `[value, from, borrows, op, output_type]`: a new node of type `from` holding `value`; with
    /// `from` `dyad`, a copy of the node another `dyad ?` place holds, where that place owns it;
    /// `borrows` is non-null where a name holds `value`, which the node then ends without.
    pub new_node: DyadPtr,
    pub new_node_leaf: DyadPtr,
    /// `[value, into, ends, lossy, op, output_type]`: the value of type `into` read out of the
    /// node `value` gives; `ends` is non-null where `value` is made there, and the read moves the
    /// value out and frees that node unless its line's end ends it; `lossy` is non-null where the
    /// node lands in `into` as in `into lossy (…)`.
    pub read: DyadPtr,
    pub read_leaf: DyadPtr,
    /// `[place, type, op, output_type]`: the record of `type` the `dyad` place `place` holds,
    /// reached through the place each time the line runs: a plain record's bytes, which lie in the
    /// node, and a record whose values are nodes, that node.
    pub reach: DyadPtr,
    pub reach_leaf: DyadPtr,
}

/// No spelling: the landing in a `dyad ?` place builds the new node, a read of a `dyad` value
/// the read.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> DyadPlaceIds {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["value", "from", "borrows", "op", "output_type"],
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
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::INERT,
        Assoc::Left,
        &["place", "type", "op", "output_type"],
    );
    let reach = cx.store.alloc_head(cx.type_, record);
    let reach_leaf = callable::mint_native(cx.store, cs.callable, run_reach, cs.seed_native);
    DyadPlaceIds { new_node, new_node_leaf, read, read_leaf, reach, reach_leaf }
}

/// Who ends what a `dyad ?` place takes (›Holding is decided at the binding site, parameters
/// included‹ [[R61]]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Holder {
    /// A field, a cell, a `mut` parameter, a `static` function's receiver: nothing ends what the
    /// place takes, so it takes only a node: stand-in for #256.
    #[default]
    Nobody,
    /// A call, for its parameter: it ends a value made or moved there, and borrows a named one,
    /// freeing the new node it lands in without the value's end (›A filled `static free` is the
    /// value's teardown; one name owns each value‹ [[R65]]).
    Call,
    /// A name, or the caller of a `-> dyad` function: it ends a value made or moved there, and a
    /// copy of a named one (›`:=` decides nothing about copy or reference; the type's reading rule
    /// does‹ [[R107]]).
    Owner,
}

/// `value` landed in a `dyad ?` place: any node the store holds as it is; where `holder` ends
/// what the place takes, a value whose type moves whole in a new node of that type, one a name
/// holds borrowed there by a call or copied by an owner, and what another `dyad ?` place holds
/// borrowed by a call or copied by an owner, unless it moves out of where it was made.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn land(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    holder: Holder,
) -> Result<DyadPtr, ParseError> {
    if types.is_hole(types.through(value)) {
        return Ok(value);
    }
    let from = super::read::output_type(types, value);
    let made = teardown_of(types, value) != Teardown::Nothing;
    if from == types.dyad_ {
        return match (holder, made) {
            (Holder::Nobody, true) => Err(ParseError::DyadTakesNode),
            (Holder::Owner, false) => Ok(build_new_node(store, types, value, from)),
            // A place no name holds borrows what another place holds, as a `static` function's
            // receiver does: stand-in for #256.
            _ => Ok(value),
        };
    }
    if holder != Holder::Nobody && is_data(types, from) {
        return Ok(match holder {
            _ if made || teardown_of_type(types, from) == Teardown::Nothing => {
                build_new_node(store, types, value, from)
            }
            Holder::Owner => {
                let copy = super::copy::taken(store, types, value);
                build_new_node(store, types, copy, from)
            }
            _ => node_of(store, types, value, from, types.dyad_),
        });
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
    // A value that is a node itself: stand-in for #295.
    if ty == types.dyad_ || !is_data(types, ty) {
        return Err(ParseError::BadDyadType);
    }
    let landed = super::land(store, types, value, ty)?;
    // The node ends what it holds, so it holds a copy of a value it would only borrow.
    let landed = super::copy::taken(store, types, landed);
    Ok(build_new_node(store, types, landed, ty))
}

/// `T ?`, parsed when its line runs since `T` is known only then: a new node of `T`, its bytes
/// zeroed, which no read reaches before a write fills it (›A type known only when the program
/// runs still works; what needs it is parsed when its line runs‹ [[R377]]).
///
/// # Safety
/// `hole` must be a hole from the store.
pub(crate) unsafe fn lay_out(
    store: &mut Store,
    types: &Core,
    hole: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let ty = super::hole::type_in(hole);
    if !is_data(types, ty) {
        return Err(ParseError::RunPlaceNotData);
    }
    Ok(build_new_node(store, types, hole, ty))
}

/// A type whose value lies in its place's bytes, a number, a pointer, a rational or a record, its
/// bytes or its address, and moves whole.
///
/// # Safety
/// `t` must be null or a valid dyad from the store.
unsafe fn is_data(types: &Core, t: DyadPtr) -> bool {
    matches!(
        place_layout(types, t),
        Some((
            Read::Scalar(_) | Read::Pointer(_) | Read::Rational | Read::Aggregate | Read::Behind(_),
            _
        ))
    ) && carry_of(types, t).is_some()
}

pub(crate) fn build_new_node(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    from: DyadPtr,
) -> DyadPtr {
    node_of(store, types, value, from, std::ptr::null_mut())
}

/// A new node of `from` holding `value`, which it borrows where `borrows` is non-null.
fn node_of(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    from: DyadPtr,
    borrows: DyadPtr,
) -> DyadPtr {
    store.alloc_words(
        types.dyad_place.new_node,
        &[value, from, borrows, types.dyad_place.new_node_leaf, types.dyad_],
    )
}

/// Whether the node `value` gives borrows the value it holds, on some path that hands it on:
/// then the one that holds the node frees it without that value's end. Every such path borrows
/// alike or ends nothing more than its bytes, since an `if` whose arms make a value in one and
/// borrow one in another is refused.
///
/// # Safety
/// `value` must be a reduced dyad from the store.
pub(crate) unsafe fn borrows(types: &Core, value: DyadPtr) -> bool {
    super::drop_model::handed_through(types, value).into_iter().any(|node| {
        dyad::ty(node) == types.dyad_place.new_node
            && !(*(dyad::value(node) as *const DyadPtr).add(BORROWS)).is_null()
    })
}

/// The slot of a new node's `borrows` mark.
const BORROWS: usize = 2;

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
/// `node` must be a node from [`build_new_node`], [`read_through`] or [`build_reach`].
unsafe fn parts(node: DyadPtr) -> (DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p, *p.add(1))
}

/// The read `node`'s two marks: whether it frees the node its value gives, one made there that no
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

/// What `.` reads a member of over `place`, a `dyad` place the parse read as `held` ([`stable`]):
/// a record the place holds is reached through the place each time the line runs, so a field
/// written through it, by `=` or by a `static` function, is written in the node the place holds
/// then (›A `dyad ?` place is transparent: a placeholder for a new node of any type‹ [[R293]]);
/// any other node, which has no field to write, is read as `held`.
///
/// # Safety
/// `place` must be a reduced dyad from the store and `held` a node from the store.
pub(crate) unsafe fn build_reach(
    store: &mut Store,
    types: &Core,
    place: DyadPtr,
    held: DyadPtr,
) -> DyadPtr {
    let ty = dyad::ty(held);
    let plain = super::this::is_plain(types, ty) && carry_of(types, ty).is_some();
    if !plain && !meta::is_node_valued(ty, types.fn_type) {
        return held;
    }
    let ids = types.dyad_place;
    store.alloc_words(ids.reach, &[place, ty, ids.reach_leaf, ty])
}

/// The line is built for a node of its `type`, so a node of another type, which a write into the
/// place put there while the line ran, is refused, never read or written by that type's layout.
fn run_reach(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node from `build_reach`; a `dyad` place gives a node's address or 0.
    unsafe {
        let (place, ty) = parts(node);
        let held = rt.run(place)? as usize as DyadPtr;
        if held.is_null() {
            return Err(RunError::NullPointer);
        }
        if dyad::ty(held) != ty {
            return Err(RunError::HeldNodeReplaced);
        }
        if super::this::in_bytes(rt.types(), ty) {
            return Ok(dyad::value(held) as i64);
        }
        if super::this::is_plain(rt.types(), ty) {
            return Ok(std::ptr::read_unaligned(dyad::value(held) as *const i64));
        }
        Ok(held as i64)
    }
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
        if rt.types().is_hole(value) {
            let node = alloc_node(rt, from, carry.width())?;
            std::ptr::write_bytes(dyad::value(node), 0, carry.width());
            return Ok(node as i64);
        }
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
        if !ends || rt.planned(value) {
            return read;
        }
        // What took the read holds the value it moves out of the node; a failed read leaves the
        // value there, which ends with it, the fault staying the error shown.
        match read {
            Ok(v) => {
                release(rt, held as i64);
                Ok(v)
            }
            Err(fault) => {
                let _ = end(rt, held as i64);
                Err(fault)
            }
        }
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
                lw.call_seed(compiled_release as *const () as usize, &[held])?;
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
    let node = alloc_node(rt, ty, carry.width())?;
    put(carry, dyad::value(node), taken);
    Ok(node as i64)
}

/// A new node of `ty` on the heap, its `width` bytes not yet written.
///
/// # Safety
/// `ty` must be a type node.
unsafe fn alloc_node(rt: &mut Runtime, ty: DyadPtr, width: usize) -> Result<DyadPtr, RunError> {
    let node = heap_alloc(PAYLOAD_OFFSET as usize + width) as DyadPtr;
    if node.is_null() {
        return Err(RunError::OutOfMemory);
    }
    dyad::set_ty(node, ty);
    rt.note_alloc();
    Ok(node)
}

/// `held` again for a place of its own: a node the store holds as it is, one a place made
/// copied.
///
/// # Safety
/// `held` must be null or a live node.
pub(crate) unsafe fn copy(rt: &mut Runtime, held: DyadPtr) -> Result<i64, RunError> {
    if !made(rt.store, held) {
        return Ok(held as i64);
    }
    let ty = dyad::ty(held);
    let carry = carry_of(rt.types(), ty).ok_or(RunError::NoWholeRead)?;
    let taken = load(carry, dyad::value(held));
    let node = make(rt, ty, carry, taken)?;
    // Each part the value owns becomes a copy of its own (›`:=` decides nothing about copy or
    // reference; the type's reading rule does‹ [[R107]]).
    if let Err(e) = super::copy::deepen(rt, ty, dyad::value(node as usize as DyadPtr)) {
        release(rt, node);
        return Err(e);
    }
    Ok(node)
}

/// The end of what a `dyad ?` place held: the node it made ends the value it holds as the
/// value's type ends one, an owning pointer's block freed and a record's `free` run over its
/// bytes, and is freed; a node the store holds is not.
///
/// # Safety
/// `word` must be 0 or the address of a live node a `dyad ?` place held.
pub(crate) unsafe fn end(rt: &mut Runtime, word: i64) -> Result<(), RunError> {
    let held = word as usize as DyadPtr;
    if !made(rt.store, held) {
        return Ok(());
    }
    let ty = dyad::ty(held);
    let ended = match carry_of(rt.types(), ty) {
        Some(carry) => end_of_type(rt, ty, load(carry, dyad::value(held))),
        None => Ok(()),
    };
    release(rt, word);
    ended
}

/// The node a `dyad ?` place made is freed, its value moved out whole; a node the store holds
/// is not.
///
/// # Safety
/// `word` must be 0 or the address of a live node a `dyad ?` place held.
pub(crate) unsafe fn release(rt: &mut Runtime, word: i64) {
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
    crate::run::interpret_step(ctx, |rt| unsafe { end(rt, word).map(|()| 0) })
}

/// # Safety
/// Called only by compiled code, with its context and `word` a node a `dyad ?` place made, its
/// value moved out.
unsafe extern "C" fn compiled_release(ctx: *mut crate::run::Context, word: i64) -> i64 {
    // SAFETY: the caller's contract.
    crate::run::interpret_step(ctx, |rt| unsafe {
        release(rt, word);
        Ok(0)
    })
}
