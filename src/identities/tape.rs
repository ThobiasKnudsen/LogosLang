// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `parsing_tape`, and the tape's affordances as identities (#60; DESIGN ›The
//! scope's constructor is the driver‹: "a data structure with exactly four
//! affordances — `tape[k]` (a read lexes lazily on demand), `is_constructed`
//! per cell, `insert`, `remove`"; ›Feasibility‹: "`tape[k]` read and write,
//! `remove`, `dyad (type, value)` construction from Logos, `fn`-typed
//! parameters carrying a tape").
//!
//! The type is a record whose one per-instance field, `cells`, is the handle
//! of the seed's [`ParsingTape`] (the sketch's `parsing_tape` names the
//! list, the flags, the center, and the source as fields; the seed keeps
//! them behind the one handle, its cells and flags being the list's own).
//! An instance is a cheap view: copying it copies the handle, sharing the
//! cells. The affordances are natives bound to the type — `t[k]` the element
//! read, a place `=` may write (`t[0] = dyad (…)` replaces the cell's pointer
//! and marks it constructed), `t.is_constructed[k]` the flag (its spelling
//! ruled 9 September 2026: the flag is the tape's own list, so the read is
//! that list's element), `t.insert(k, cell)`, `t.remove(k)`, `t.recenter(k)`.
//! A member call passes its receiver as an address expression, the seed's
//! form of a member declared in the instance scope reading the instance's
//! fields bare (›The constructor is a field‹). A cell handed in — `insert`'s
//! cell, the value of a write — travels as an `@dyad` address value: a
//! record for a use of a name (the cell stays unconstructed), a node
//! otherwise (constructed). The natives run interpreted; a read past the
//! lexed frontier at run time is the checked error until `lex` (#62) puts
//! the lexer in the runtime. Nothing here lowers.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::{Cell, CoreTypes, ParseError, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;

/// The handles: the type, and each native's identity with its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct TapeIds {
    pub parsing_tape: DyadPtr,
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    pub is_constructed: DyadPtr,
    pub is_constructed_leaf: DyadPtr,
    pub insert: DyadPtr,
    pub insert_leaf: DyadPtr,
    pub remove: DyadPtr,
    pub remove_leaf: DyadPtr,
    pub recenter: DyadPtr,
    pub recenter_leaf: DyadPtr,
}

/// Register `parsing_tape` and its natives. The members are declared in the
/// type's own scope, where `.` resolves them; the type's field list holds
/// only `cells`, so an instance is eight bytes.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    void_ty: DyadPtr,
) -> TapeIds {
    let scope = cx.store.alloc_raw(scope_ty, std::ptr::null_mut());
    let at_void = super::pointer::make_pointer_type(cx.store, cx.type_, void_ty);
    let cells = cx.store.alloc_raw(at_void, std::ptr::null_mut());
    cx.declare_in(scope, "cells", cells);
    let fields = super::array::build(cx.store, array_ty, &[cells]);
    let layout = meta::record_layout(cx.store, scope, fields, 8);
    let parsing_tape = cx.store.alloc_raw(cx.type_, layout);
    cx.declare("parsing_tape", parsing_tape);

    let op = |cx: &mut Cx, roles: &[&str], run: crate::run::RunFn| {
        let record = meta::operand_record(
            cx,
            meta::TUPLE_TAG,
            meta::prec::INERT,
            crate::parse::Assoc::Left,
            roles,
        );
        let id = cx.store.alloc_raw(cx.type_, record);
        let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
        (id, leaf)
    };
    let (slot, slot_leaf) = op(cx, &["tape", "k", "op"], run_slot);
    let (write, write_leaf) = op(cx, &["tape", "k", "cell", "op"], run_write);
    let (is_constructed, is_constructed_leaf) = op(cx, &["tape", "k", "op"], run_is_constructed);
    let (insert, insert_leaf) = op(cx, &["tape", "k", "cell", "op"], run_insert);
    let (remove, remove_leaf) = op(cx, &["tape", "k", "op"], run_remove);
    let (recenter, recenter_leaf) = op(cx, &["tape", "k", "op"], run_recenter);
    for (name, id) in [
        ("is_constructed", is_constructed),
        ("insert", insert),
        ("remove", remove),
        ("recenter", recenter),
    ] {
        cx.declare_in(scope, name, id);
    }
    TapeIds {
        parsing_tape,
        slot,
        slot_leaf,
        write,
        write_leaf,
        is_constructed,
        is_constructed_leaf,
        insert,
        insert_leaf,
        remove,
        remove_leaf,
        recenter,
        recenter_leaf,
    }
}

/// The member `name` of `parsing_tape`, if it is one of the natives.
pub(crate) fn member(ids: &TapeIds, name: &str) -> Option<(DyadPtr, DyadPtr)> {
    match name {
        "is_constructed" => Some((ids.is_constructed, ids.is_constructed_leaf)),
        "insert" => Some((ids.insert, ids.insert_leaf)),
        "remove" => Some((ids.remove, ids.remove_leaf)),
        "recenter" => Some((ids.recenter, ids.recenter_leaf)),
        _ => None,
    }
}

/// The receiver of a tape read or call as an address expression, or `None`
/// when `lhs` is no tape: an instance place gives `&place`, a dereference of
/// a `@parsing_tape` gives its pointer expression.
///
/// # Safety
/// `lhs` must be a reduced dyad from the store.
pub(crate) unsafe fn receiver_addr(store: &mut Store, types: &CoreTypes, lhs: DyadPtr) -> Option<DyadPtr> {
    if (*lhs).ty == types.deref_ {
        let (ptr_expr, pointee, off) = super::pointer::deref_parts(lhs);
        if pointee == types.tape.parsing_tape && off == 0 {
            return Some(ptr_expr);
        }
        return None;
    }
    if (*lhs).ty == types.tape.parsing_tape && !(*lhs).value.is_null() {
        return Some(super::pointer::build_addr(store, types, lhs));
    }
    None
}

fn node(store: &mut Store, op: DyadPtr, leaf: DyadPtr, operands: &[DyadPtr]) -> DyadPtr {
    let mut v = operands.to_vec();
    v.push(leaf);
    let value = store.alloc_operands(&v);
    store.alloc_raw(op, value)
}

/// `t[k]`: the element read, a slot node `[tape, k, op]` — read, the cell's
/// pointer; as `=`'s target, the write ([`build_write`]).
pub(crate) fn build_slot(store: &mut Store, types: &CoreTypes, recv: DyadPtr, k: DyadPtr) -> DyadPtr {
    node(store, types.tape.slot, types.tape.slot_leaf, &[recv, k])
}

/// `t[k] = cell`: the write behind a slot target.
///
/// # Safety
/// `slot` must be a slot node from [`build_slot`].
pub(crate) unsafe fn build_write(store: &mut Store, types: &CoreTypes, slot: DyadPtr, cell: DyadPtr) -> DyadPtr {
    let ops = (*slot).value as *const DyadPtr;
    let (recv, k) = (*ops, *ops.add(1));
    let cell = super::pointer::address_value(store, types, types.dyad_, cell);
    node(store, types.tape.write, types.tape.write_leaf, &[recv, k, cell])
}

/// A member call or indexed member read on a tape: `t.remove(k)`,
/// `t.insert(k, cell)`, `t.recenter(k)`, `t.is_constructed[k]`.
///
/// # Safety
/// `args` must be reduced dyads from the store.
pub(crate) unsafe fn build_member(
    store: &mut Store,
    types: &CoreTypes,
    recv: DyadPtr,
    op: DyadPtr,
    leaf: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let ids = &types.tape;
    if op == ids.insert {
        let [k, cell] = args[..] else {
            return Err(ParseError::CtorArity);
        };
        let cell = super::pointer::address_value(store, types, types.dyad_, cell);
        return Ok(node(store, op, leaf, &[recv, k, cell]));
    }
    let [k] = args[..] else {
        return Err(ParseError::CtorArity);
    };
    Ok(node(store, op, leaf, &[recv, k]))
}

/// The tape a native's receiver operand names: the instance's address, read
/// for its `cells` handle.
unsafe fn tape_of(rt: &mut Runtime, recv: DyadPtr) -> Result<*mut ParsingTape, RunError> {
    let addr = rt.run(recv)? as *const u8;
    if addr.is_null() {
        return Err(RunError::BadValue);
    }
    let tape = std::ptr::read_unaligned(addr as *const *mut ParsingTape);
    if tape.is_null() {
        return Err(RunError::BadValue);
    }
    Ok(tape)
}

/// A cell handed in as an address value: a record stays unconstructed (a
/// use of a name), a node is constructed.
unsafe fn cell_of(rt: &Runtime, dyad: DyadPtr) -> Cell {
    let constructed = !dyad.is_null() && (*dyad).ty != rt.record_ty();
    Cell { dyad, constructed, bracket: false, start: 0, len: 0 }
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        // A read past the frontier lexes lazily at parse time; at run time,
        // with no lexer in the runtime (#62), it is the checked error.
        match (*tape).at(k) {
            Some(c) => Ok(c.dyad as i64),
            None => Err(RunError::BadValue),
        }
    }
}

fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let dyad = rt.run(*ops.add(2))? as DyadPtr;
        let cell = cell_of(rt, dyad);
        let Some(c) = (*tape).at_mut(k) else {
            return Err(RunError::BadValue);
        };
        c.dyad = cell.dyad;
        c.constructed = cell.constructed;
        c.bracket = false;
        Ok(0)
    }
}

fn run_is_constructed(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        match (*tape).is_constructed(k) {
            Some(flag) => Ok(i64::from(flag)),
            None => Err(RunError::BadValue),
        }
    }
}

fn run_insert(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let dyad = rt.run(*ops.add(2))? as DyadPtr;
        let cell = cell_of(rt, dyad);
        (*tape).insert(k, cell);
        Ok(0)
    }
}

fn run_remove(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        Ok((*tape).remove(k).map(|c| c.dyad as i64).unwrap_or(0))
    }
}

fn run_recenter(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        (*tape).recenter(k);
        Ok(0)
    }
}
