// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `parsing_tape` and the tape's affordances as identities: `t[k]` read and write,
//! `t.is_constructed[k]`, `t.spelling[k]`, `t.insert`, `t.remove`, `t.recenter`, and
//! the cell reads `t[k]:name` and `t[k]:type`. The type's one field,
//! `cells`, is the seed's `ParsingTape` handle. The natives run interpreted; nothing lowers.

use super::callable::{self, Callables};
use super::{meta, numtype_of, Cx, Operand};
use crate::dyad::DyadPtr;
use crate::parse::{ParseError, ParsingTape};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

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
    /// `t.is_constructed[k] = flag`: the write behind the flag read.
    pub flag_write: DyadPtr,
    pub flag_write_leaf: DyadPtr,
    /// `t.spelling[k]`: the text the cell was lexed from, as a string node.
    pub spelling: DyadPtr,
    pub spelling_leaf: DyadPtr,
    pub insert: DyadPtr,
    pub insert_leaf: DyadPtr,
    pub remove: DyadPtr,
    pub remove_leaf: DyadPtr,
    pub recenter: DyadPtr,
    pub recenter_leaf: DyadPtr,
    /// `t[k]:name`: the spelling of the record the cell holds, a string node.
    pub slot_name: DyadPtr,
    pub slot_name_leaf: DyadPtr,
    /// `t[k]:type`: the type of the dyad the cell names, read.
    pub cell_type: DyadPtr,
    pub cell_type_leaf: DyadPtr,
}

/// The members are declared in the type's own scope, where `.` resolves them; the
/// field list holds only `cells`, so an instance is eight bytes.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    void_ty: DyadPtr,
) -> TapeIds {
    let scope = cx.store.alloc_raw(scope_ty, std::ptr::null_mut());
    // SAFETY: `void_ty` is the type node `Core::build` minted.
    let at_void = unsafe { super::pointer::make_pointer_type(cx.store, cx.type_, void_ty) };
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
    let (flag_write, flag_write_leaf) = op(cx, &["tape", "k", "flag", "op"], run_flag_write);
    let (spelling, spelling_leaf) = op(cx, &["tape", "k", "op"], run_spelling);
    let (insert, insert_leaf) = op(cx, &["tape", "k", "cells", "op"], run_insert);
    let (remove, remove_leaf) = op(cx, &["tape", "k", "op"], run_remove);
    let (recenter, recenter_leaf) = op(cx, &["tape", "k", "op"], run_recenter);
    let (slot_name, slot_name_leaf) = op(cx, &["tape", "k", "op"], run_slot_name);
    let (cell_type, cell_type_leaf) = op(cx, &["tape", "k", "op"], run_cell_type);
    for (name, id) in [
        ("is_constructed", is_constructed),
        ("spelling", spelling),
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
        flag_write,
        flag_write_leaf,
        spelling,
        spelling_leaf,
        insert,
        insert_leaf,
        remove,
        remove_leaf,
        recenter,
        recenter_leaf,
        slot_name,
        slot_name_leaf,
        cell_type,
        cell_type_leaf,
    }
}

pub(crate) fn member(ids: &TapeIds, name: &str) -> Option<(DyadPtr, DyadPtr)> {
    match name {
        "is_constructed" => Some((ids.is_constructed, ids.is_constructed_leaf)),
        "spelling" => Some((ids.spelling, ids.spelling_leaf)),
        "insert" => Some((ids.insert, ids.insert_leaf)),
        "remove" => Some((ids.remove, ids.remove_leaf)),
        "recenter" => Some((ids.recenter, ids.recenter_leaf)),
        _ => None,
    }
}

/// The receiver as an address expression, or `None` when `lhs` is no tape: an instance
/// place gives `&place`, a dereference of a `@parsing_tape` its pointer expression.
///
/// # Safety
/// `lhs` must be a reduced dyad from the store.
pub(crate) unsafe fn receiver_addr(
    store: &mut Store,
    types: &Core,
    lhs: DyadPtr,
) -> Option<DyadPtr> {
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

/// `t[k]`: read, the cell's pointer; as `=`'s target, the write (`build_write`).
pub(crate) fn build_slot(store: &mut Store, types: &Core, recv: DyadPtr, k: DyadPtr) -> DyadPtr {
    node(store, types.tape.slot, types.tape.slot_leaf, &[recv, k])
}

/// A value that already yields a cell's address passes as it stands; any other node is
/// handed by identity, its own address as an `@dyad` value.
fn cell_arg(store: &mut Store, types: &Core, cell: DyadPtr) -> DyadPtr {
    // SAFETY: `cell` is a reduced dyad from the store.
    let yields_node = unsafe {
        matches!(numtype_of(types, cell), Operand::Pointer(p) if p == types.dyad_)
            || matches!(
                super::read::read_kind(types, types.through(cell)),
                super::read::Read::Container(t) if t == types.dyad_
            )
    };
    if yields_node {
        cell
    } else {
        super::pointer::address_value(store, types, types.dyad_, cell)
    }
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_write(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    cell: DyadPtr,
) -> DyadPtr {
    let ops = (*slot).value as *const DyadPtr;
    let (recv, k) = (*ops, *ops.add(1));
    let cell = cell_arg(store, types, cell);
    node(store, types.tape.write, types.tape.write_leaf, &[recv, k, cell])
}

/// # Safety
/// `over` must be a slot node or a node built from one here.
unsafe fn slot_parts(over: DyadPtr) -> (DyadPtr, DyadPtr) {
    let ops = (*over).value as *const DyadPtr;
    (*ops, *ops.add(1))
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_slot_name(store: &mut Store, types: &Core, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.slot_name, types.tape.slot_name_leaf, &[recv, k])
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_cell_type(store: &mut Store, types: &Core, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.cell_type, types.tape.cell_type_leaf, &[recv, k])
}

/// `flag` is a bool, checked by `=`.
///
/// # Safety
/// `flag_read` must be a node `build_member` built for `is_constructed`.
pub(crate) unsafe fn build_flag_write(
    store: &mut Store,
    types: &Core,
    flag_read: DyadPtr,
    flag: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(flag_read);
    node(store, types.tape.flag_write, types.tape.flag_write_leaf, &[recv, k, flag])
}

/// `insert` takes a tape: a `lex «…»` node as it stands, or a `parsing_tape` place by
/// its address; anything else is the checked error.
///
/// # Safety
/// `args` must be reduced dyads from the store.
pub(crate) unsafe fn build_member(
    store: &mut Store,
    types: &Core,
    recv: DyadPtr,
    op: DyadPtr,
    leaf: DyadPtr,
    args: &[DyadPtr],
) -> Result<DyadPtr, ParseError> {
    let ids = &types.tape;
    if op == ids.insert {
        let [k, cells] = args[..] else {
            return Err(ParseError::CtorArity);
        };
        let cells = if super::lex::is_fragment(types, cells) {
            cells
        } else {
            let d = types.through(cells);
            receiver_addr(store, types, d).ok_or(ParseError::InsertTakesTape)?
        };
        return Ok(node(store, op, leaf, &[recv, k, cells]));
    }
    let [k] = args[..] else {
        return Err(ParseError::CtorArity);
    };
    Ok(node(store, op, leaf, &[recv, k]))
}

/// The instance's address, read for its `cells` handle.
unsafe fn tape_of(rt: &mut Runtime, recv: DyadPtr) -> Result<*mut ParsingTape, RunError> {
    let addr = rt.run(recv)? as *const u8;
    if addr.is_null() {
        return Err(RunError::NoTape);
    }
    let tape = std::ptr::read_unaligned(addr as *const *mut ParsingTape);
    if tape.is_null() {
        return Err(RunError::NoTape);
    }
    Ok(tape)
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        // Past the frontier a parse-time read lexes lazily; at run time it is the checked error.
        match (*tape).at(k) {
            Some(c) => Ok(c.dyad as i64),
            None => Err(RunError::OffTape),
        }
    }
}

/// The pointer replaced and nothing more; the flag is the constructor's line.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let dyad = rt.run(*ops.add(2))? as DyadPtr;
        if !(*tape).set_dyad(k, dyad) {
            return Err(RunError::OffTape);
        }
        Ok(0)
    }
}

fn run_flag_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let flag = rt.run(*ops.add(2))? != 0;
        if !(*tape).set_constructed(k, flag) {
            return Err(RunError::OffTape);
        }
        Ok(0)
    }
}

fn run_is_constructed(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        match (*tape).is_constructed(k) {
            Some(flag) => Ok(i64::from(flag)),
            None => Err(RunError::OffTape),
        }
    }
}

/// A cell nothing lexed answers the empty text. The string node is built into the
/// parser's store, so the read needs one attached.
fn run_spelling(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let Some(text) = (*tape).spelling(k) else {
            return Err(RunError::OffTape);
        };
        let string_ty = rt.types().string_;
        let store = rt.store();
        Ok(super::string::build_text(store, string_ty, text.as_bytes()) as i64)
    }
}

/// The fragment's cells are copied out first, so a tape spliced into itself is well-defined.
fn run_insert(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let arg = *ops.add(2);
        let frag = if super::lex::is_fragment(rt.types(), arg) {
            rt.run(arg)? as *mut ParsingTape
        } else {
            tape_of(rt, arg)?
        };
        if frag.is_null() {
            return Err(RunError::NoFragment);
        }
        let cells = (*frag).cells();
        (*tape).splice(k, cells);
        Ok(0)
    }
}

fn run_remove(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        Ok((*tape).remove(k).map(|c| c.dyad as i64).unwrap_or(0))
    }
}

fn run_recenter(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        (*tape).recenter(k);
        Ok(0)
    }
}

/// The cell, read through a record to the dyad it names; `None` past the frontier.
unsafe fn slot_cell(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<Option<(*mut ParsingTape, isize, DyadPtr)>, RunError> {
    let tape = tape_of(rt, *ops)?;
    let k = rt.run(*ops.add(1))? as isize;
    Ok((*tape).at(k).map(|c| (tape, k, rt.through(c.dyad))))
}

/// The identity's name, never the appearance's text (that is `t.spelling[k]`); a cell
/// holding no record has no name.
fn run_slot_name(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let tape = tape_of(rt, *ops)?;
        let k = rt.run(*ops.add(1))? as isize;
        let Some(c) = (*tape).at(k).copied() else {
            return Err(RunError::OffTape);
        };
        let record = c.record(rt.types());
        if record.is_null() {
            return Err(RunError::NoName);
        }
        let name = crate::record::Record::read(record).name;
        if name.is_null() {
            return Err(RunError::NoName);
        }
        Ok(name as i64)
    }
}

fn run_cell_type(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        match slot_cell(rt, ops)? {
            Some((_, _, cell)) => Ok((*cell).ty as i64),
            None => Err(RunError::OffTape),
        }
    }
}
