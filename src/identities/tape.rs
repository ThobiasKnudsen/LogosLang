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
use super::{meta, numtype_of, Cx, Operand};
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
    /// `t[k]:dyad`: the cell the slot holds, a record read through to the
    /// dyad it names (#61).
    pub slot_dyad: DyadPtr,
    pub slot_dyad_leaf: DyadPtr,
    /// `t[k]:dyad.type`: the cell's type, read; as `=`'s target, the retype.
    pub cell_type: DyadPtr,
    pub cell_type_leaf: DyadPtr,
    /// `t[k]:dyad.value` and `.value.operands`: the steps of the path to the
    /// cell's operand record, markers the next read consumes; standing alone
    /// they run to nothing.
    pub cell_value: DyadPtr,
    pub cell_operands: DyadPtr,
    /// `t[k]:dyad.type = T`: a fresh cell of type `T` with an empty operand
    /// record, written into the slot.
    pub retype: DyadPtr,
    pub retype_leaf: DyadPtr,
    /// `t[k]:dyad.value.operands.append(…)`: the operand record grown by the
    /// cells appended.
    pub append: DyadPtr,
    pub append_leaf: DyadPtr,
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
    let layout = meta::record_layout(
        cx.store,
        scope,
        fields,
        8,
        std::ptr::null_mut(),
        meta::prec::APPLY,
        crate::parse::Assoc::Left,
    );
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
    let (slot_dyad, slot_dyad_leaf) = op(cx, &["tape", "k", "op"], run_slot_dyad);
    let (cell_type, cell_type_leaf) = op(cx, &["tape", "k", "op"], run_cell_type);
    let (retype, retype_leaf) = op(cx, &["tape", "k", "type", "op"], run_retype);
    // The path markers carry the slot and run to nothing: no leaf.
    let marker = |cx: &mut Cx| {
        let record = meta::operand_record(
            cx,
            meta::TUPLE_TAG,
            meta::prec::INERT,
            crate::parse::Assoc::Left,
            &["tape", "k", "op"],
        );
        cx.store.alloc_raw(cx.type_, record)
    };
    let cell_value = marker(cx);
    let cell_operands = marker(cx);
    // `append` is variadic: `[tape, k, op, cell…, null]`, the op slot fixed
    // at 2 and the cells the list's tail.
    let record = meta::operand_record(
        cx,
        meta::LIST_TAG,
        meta::prec::INERT,
        crate::parse::Assoc::Left,
        &["tape", "k", "op"],
    );
    let append = cx.store.alloc_raw(cx.type_, record);
    let append_leaf = callable::mint_native(cx.store, cs.callable, run_append, cs.seed_native);
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
        slot_dyad,
        slot_dyad_leaf,
        cell_type,
        cell_type_leaf,
        cell_value,
        cell_operands,
        retype,
        retype_leaf,
        append,
        append_leaf,
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

/// A cell handed to a native: a value that already yields a cell's address
/// (a slot read, a cell read through it) passes as it stands; any other node
/// is handed *by identity*, its own address as an `@dyad` value.
fn cell_arg(store: &mut Store, types: &CoreTypes, cell: DyadPtr) -> DyadPtr {
    // SAFETY: `cell` is a reduced dyad from the store.
    if matches!(unsafe { numtype_of(types, cell) }, Operand::Pointer(p) if p == types.dyad_) {
        cell
    } else {
        super::pointer::address_value(store, types, types.dyad_, cell)
    }
}

/// `t[k] = cell`: the write behind a slot target.
///
/// # Safety
/// `slot` must be a slot node from [`build_slot`].
pub(crate) unsafe fn build_write(store: &mut Store, types: &CoreTypes, slot: DyadPtr, cell: DyadPtr) -> DyadPtr {
    let ops = (*slot).value as *const DyadPtr;
    let (recv, k) = (*ops, *ops.add(1));
    let cell = cell_arg(store, types, cell);
    node(store, types.tape.write, types.tape.write_leaf, &[recv, k, cell])
}

/// The slot a node built over one names: its `[tape, k]` head.
///
/// # Safety
/// `over` must be a slot node or a node built from one here.
unsafe fn slot_parts(over: DyadPtr) -> (DyadPtr, DyadPtr) {
    let ops = (*over).value as *const DyadPtr;
    (*ops, *ops.add(1))
}

/// `t[k]:dyad`: the cell the slot holds (#61).
///
/// # Safety
/// `slot` must be a slot node from [`build_slot`].
pub(crate) unsafe fn build_slot_dyad(store: &mut Store, types: &CoreTypes, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.slot_dyad, types.tape.slot_dyad_leaf, &[recv, k])
}

/// `t[k]:dyad.type`: the cell's type.
///
/// # Safety
/// `slot_dyad` must be a node from [`build_slot_dyad`].
pub(crate) unsafe fn build_cell_type(store: &mut Store, types: &CoreTypes, slot_dyad: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot_dyad);
    node(store, types.tape.cell_type, types.tape.cell_type_leaf, &[recv, k])
}

/// `t[k]:dyad.value` or `.value.operands`: a step of the path, carrying the
/// slot for the read that completes it.
///
/// # Safety
/// `over` must be a node from [`build_slot_dyad`] or this builder.
pub(crate) unsafe fn build_cell_marker(store: &mut Store, marker: DyadPtr, over: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(over);
    node(store, marker, std::ptr::null_mut(), &[recv, k])
}

/// `t[k]:dyad.type = T`: the retype behind a cell-type target. `T` is
/// stored as it stands — a use of a name is its record — and read through at
/// run, so a type named inside its own definition body resolves to the
/// finished type when the constructor runs.
///
/// # Safety
/// `cell_type` must be a node from [`build_cell_type`].
pub(crate) unsafe fn build_retype(store: &mut Store, types: &CoreTypes, cell_type: DyadPtr, ty: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(cell_type);
    node(store, types.tape.retype, types.tape.retype_leaf, &[recv, k, ty])
}

/// `t[k]:dyad.value.operands.append(cell, …)`: `[tape, k, op, cell…, null]`.
///
/// # Safety
/// `cell_operands` must be a node from [`build_cell_marker`]; `items` reduced
/// dyads from the store.
pub(crate) unsafe fn build_append(
    store: &mut Store,
    types: &CoreTypes,
    cell_operands: DyadPtr,
    items: &[DyadPtr],
) -> DyadPtr {
    let (recv, k) = slot_parts(cell_operands);
    let mut v = vec![recv, k, types.tape.append_leaf];
    for &item in items {
        v.push(cell_arg(store, types, item));
    }
    v.push(std::ptr::null_mut());
    let value = store.alloc_operands(&v);
    store.alloc_raw(types.tape.append, value)
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
        let cell = cell_arg(store, types, cell);
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

/// The cell a `[tape, k, …]` node's slot holds, read through a record to the
/// dyad it names (the reading rule): the constructed node, or the identity a
/// spelling resolved to. `None` past the frontier.
unsafe fn slot_cell(rt: &mut Runtime, ops: *const DyadPtr) -> Result<Option<(*mut ParsingTape, isize, DyadPtr)>, RunError> {
    let tape = tape_of(rt, *ops)?;
    let k = rt.run(*ops.add(1))? as isize;
    Ok((*tape).at(k).map(|c| (tape, k, rt.through(c.dyad))))
}

fn run_slot_dyad(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        match slot_cell(rt, ops)? {
            Some((_, _, cell)) => Ok(cell as i64),
            None => Err(RunError::BadValue),
        }
    }
}

fn run_cell_type(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        match slot_cell(rt, ops)? {
            Some((_, _, cell)) => Ok((*cell).ty as i64),
            None => Err(RunError::BadValue),
        }
    }
}

/// `t[k]:dyad.type = T` (ruled 9 September 2026: "when assigning the type of
/// a dyad the value should be automatically initialized so that .operands is
/// valid and available"): a fresh cell of type `T` whose value is an empty
/// operand record — the null-terminated run a call's arguments travel in,
/// the shape a `code`-carrying type runs as (#63) — replaces what the slot
/// held, constructed. A write through the identity the slot pointed at would
/// reclassify it for the whole program (DESIGN ›The scope's constructor is
/// the driver‹), so the cell is new.
fn run_retype(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let ty = rt.through(*ops.add(2));
        let store = rt.store()?;
        let run = store.alloc_operands(&[std::ptr::null_mut()]);
        let cell = store.alloc_raw(ty, run);
        let Some(c) = (*tape).at_mut(k) else {
            return Err(RunError::BadValue);
        };
        c.dyad = cell;
        c.constructed = true;
        c.bracket = false;
        Ok(cell as i64)
    }
}

/// `t[k]:dyad.value.operands.append(cell, …)`: the constructed cell's operand
/// run, grown by the cells given — each as it stands, the record for a use
/// of a name (DESIGN ›The dyad's read surface‹: a use points at its record).
/// The run is reallocated whole; the old one stays in the store, as every
/// superseded node does.
fn run_append(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let Some(c) = (*tape).at(k).copied() else {
            return Err(RunError::BadValue);
        };
        if !c.constructed {
            return Err(RunError::BadValue);
        }
        let cell = c.dyad;
        let mut run = Vec::new();
        let old = (*cell).value as *const DyadPtr;
        if !old.is_null() {
            let mut i = 0;
            while !(*old.add(i)).is_null() {
                run.push(*old.add(i));
                i += 1;
            }
        }
        let mut i = 3;
        while !(*ops.add(i)).is_null() {
            run.push(rt.run(*ops.add(i))? as DyadPtr);
            i += 1;
        }
        run.push(std::ptr::null_mut());
        (*cell).value = rt.store()?.alloc_operands(&run);
        Ok(0)
    }
}
