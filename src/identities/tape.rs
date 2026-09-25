// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `parsing_tape` and the tape's affordances as identities: `t[k]` read and write,
//! `t.is_constructed[k]`, `t.spelling[k]`, `t.insert`, `t.remove`, `t.recenter`, and
//! the cell reads `t[k]:name`, `t[k]:type`, a `t[k]` checked to hold a type as that
//! type or checked against a number type as that number, and a scope or `[…]` cell's
//! `t[k].dyads`, `t[k].dyads.size` and `t[k].dyads[i]`, whose line reads as a cell does,
//! and a call written into a cell, placed with its tape lines as operands. The type's one field,
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
    /// `t[k]:name`: the spelling of the binding the cell holds, a string node.
    pub slot_name: DyadPtr,
    pub slot_name_leaf: DyadPtr,
    /// `t[k]:type`: the type of the dyad the cell names, read; `[tape, k, i]`, `i` null
    /// for the cell itself or the line `t[k].dyads[i]`, as in the two reads below.
    pub cell_type: DyadPtr,
    pub cell_type_leaf: DyadPtr,
    /// `t[k]` after a check that the cell holds a type: the type it holds.
    pub cell_value: DyadPtr,
    pub cell_value_leaf: DyadPtr,
    /// `t[k]` after a check against a number type: `[tape, k, i, type]`, the number it holds.
    pub cell_number: DyadPtr,
    pub cell_number_leaf: DyadPtr,
    /// `t[k].dyads`: the scope cell's `dyads` array, as its address.
    pub cell_dyads: DyadPtr,
    pub cell_dyads_leaf: DyadPtr,
    /// `t[k].dyads.size`, a `u64`.
    pub cell_dyads_size: DyadPtr,
    pub cell_dyads_size_leaf: DyadPtr,
    /// `t[k].dyads[i]`: the scope cell's line i, as its address.
    pub cell_dyad_at: DyadPtr,
    pub cell_dyad_at_leaf: DyadPtr,
    /// A call a constructor places, `[call, op]`: yields a copy of the call whose arguments
    /// are the tape lines they read, as operands, and the values of the others.
    pub placed_call: DyadPtr,
    pub placed_call_leaf: DyadPtr,
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
    let (cell_type, cell_type_leaf) = op(cx, &["tape", "k", "i", "op"], run_cell_type);
    let (cell_value, cell_value_leaf) = op(cx, &["tape", "k", "i", "op"], run_cell_value);
    let (cell_number, cell_number_leaf) =
        op(cx, &["tape", "k", "i", "type", "op"], run_cell_number);
    let (cell_dyads, cell_dyads_leaf) = op(cx, &["tape", "k", "op"], run_cell_dyads);
    let (cell_dyads_size, cell_dyads_size_leaf) = op(cx, &["tape", "k", "op"], run_cell_dyads_size);
    let (cell_dyad_at, cell_dyad_at_leaf) = op(cx, &["tape", "k", "i", "op"], run_cell_dyad_at);
    let (placed_call, placed_call_leaf) = op(cx, &["call", "op"], run_placed_call);
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
        cell_value,
        cell_value_leaf,
        cell_number,
        cell_number_leaf,
        cell_dyads,
        cell_dyads_leaf,
        cell_dyads_size,
        cell_dyads_size_leaf,
        cell_dyad_at,
        cell_dyad_at_leaf,
        placed_call,
        placed_call_leaf,
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

/// A value that already yields a node's address, a dyad or a type, passes as it stands;
/// any other node is handed by identity, its own address as an `@dyad` value.
pub(crate) fn cell_arg(store: &mut Store, types: &Core, cell: DyadPtr) -> DyadPtr {
    // A field read by value is handed as the node the field holds, which yields that value.
    // SAFETY: `cell` is a reduced dyad from the store; a load node's operands are `[this, k, type]`.
    unsafe {
        if (*cell).ty == types.this.load {
            let ops = (*cell).value as *const DyadPtr;
            return super::this::build_slot(store, types, *ops, *ops.add(1));
        }
    }
    // SAFETY: `cell` is a reduced dyad from the store.
    let yields_node = unsafe {
        matches!(numtype_of(types, cell), Operand::Pointer(p) if p == types.dyad_)
            || matches!(
                super::read::read_kind(types, types.through(cell)),
                super::read::Read::Container(t) if t == types.dyad_
            )
            || super::yields_type(types, cell)
    };
    if yields_node {
        return cell;
    }
    // SAFETY: as above.
    let read = unsafe { super::read::read_kind(types, types.through(cell)) };
    if let super::read::Read::Executable(super::read::Dispatch::Call(_)) = read {
        return node(store, types.tape.placed_call, types.tape.placed_call_leaf, &[cell]);
    }
    super::pointer::address_value(store, types, types.dyad_, cell)
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

/// The tape, the cell index, and the line index of a line read, null for the cell itself.
///
/// # Safety
/// `read` must be a slot node, a line node from `build_cell_dyad_at`, or a node built
/// from either by the builders below.
unsafe fn read_parts(types: &Core, read: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let (recv, k) = slot_parts(read);
    let ty = (*read).ty;
    let has_line = ty == types.tape.cell_dyad_at
        || ty == types.tape.cell_type
        || ty == types.tape.cell_value
        || ty == types.tape.cell_number;
    let i = if has_line { *((*read).value as *const DyadPtr).add(2) } else { std::ptr::null_mut() };
    (recv, k, i)
}

/// `t[k]:type` or `t[k].dyads[i]:type`.
///
/// # Safety
/// As `read_parts`.
pub(crate) unsafe fn build_cell_type(store: &mut Store, types: &Core, read: DyadPtr) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    node(store, types.tape.cell_type, types.tape.cell_type_leaf, &[recv, k, i])
}

/// DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
///
/// # Safety
/// As `read_parts`.
pub(crate) unsafe fn build_cell_value(store: &mut Store, types: &Core, read: DyadPtr) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    node(store, types.tape.cell_value, types.tape.cell_value_leaf, &[recv, k, i])
}

/// The same paragraph, a cell or line checked against the number type `ty`.
///
/// # Safety
/// As `read_parts`; `ty` a number type node.
pub(crate) unsafe fn build_cell_number(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    node(store, types.tape.cell_number, types.tape.cell_number_leaf, &[recv, k, i, ty])
}

/// A read of the cell itself, plain or checked: what `=`, `:` and `.dyads` take.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_cell_read(types: &Core, node: DyadPtr) -> bool {
    (*node).ty == types.tape.slot || (*node).ty == types.tape.cell_value
}

/// Which line of a cell a read names: none, a literal index, or the place an index is
/// read from, one name in the check and the read naming one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    Cell,
    At(i32),
    Named(DyadPtr),
}

/// Which cell a read built by this file names, as the tape's place, a literal index, and
/// the line; `None` for a cell index known only at run.
///
/// # Safety
/// As `read_parts`.
pub(crate) unsafe fn cell_key(types: &Core, over: DyadPtr) -> Option<(DyadPtr, i32, Line)> {
    let (recv, k, i) = read_parts(types, over);
    let recv = if (*recv).ty == types.addr_ {
        types.through(*((*recv).value as *const DyadPtr))
    } else {
        types.through(recv)
    };
    let k = types.through(k);
    if (*k).ty != types.rational {
        return None;
    }
    let line = if i.is_null() {
        Line::Cell
    } else {
        let i = types.through(i);
        if (*i).ty == types.rational {
            Line::At(super::rational::mold(i)?)
        } else {
            Line::Named(i)
        }
    };
    super::rational::mold(k).map(|k| (recv, k, line))
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_cell_dyads(store: &mut Store, types: &Core, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.cell_dyads, types.tape.cell_dyads_leaf, &[recv, k])
}

/// # Safety
/// `dyads` must be a node from `build_cell_dyads`.
pub(crate) unsafe fn build_cell_dyads_size(
    store: &mut Store,
    types: &Core,
    dyads: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(dyads);
    node(store, types.tape.cell_dyads_size, types.tape.cell_dyads_size_leaf, &[recv, k])
}

/// # Safety
/// `slot` must be a slot node from `build_slot`; `i` a reduced dyad.
pub(crate) unsafe fn build_cell_dyad_at(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    i: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.cell_dyad_at, types.tape.cell_dyad_at_leaf, &[recv, k, i])
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

/// The tape and the index a native reads at, the tape lexed on to that cell first.
unsafe fn tape_at(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<(*mut ParsingTape, isize), RunError> {
    let tape = tape_of(rt, *ops)?;
    let k = rt.run(*ops.add(1))? as isize;
    rt.reach(tape, k)?;
    Ok((tape, k))
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
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
        let (tape, k) = tape_at(rt, ops)?;
        Ok((*tape).remove(k).map(|c| c.dyad as i64).unwrap_or(0))
    }
}

fn run_recenter(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        (*tape).recenter(k);
        Ok(0)
    }
}

/// The cell, read through a binding to the dyad it names; `None` past the frontier.
unsafe fn slot_cell(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<Option<(*mut ParsingTape, isize, DyadPtr)>, RunError> {
    let (tape, k) = tape_at(rt, ops)?;
    Ok((*tape).at(k).map(|c| (tape, k, rt.through(c.dyad))))
}

/// The identity's name, never the appearance's text (that is `t.spelling[k]`); a cell
/// holding no binding has no name.
fn run_slot_name(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        let Some(c) = (*tape).at(k).copied() else {
            return Err(RunError::OffTape);
        };
        let binding = c.binding(rt.types());
        if binding.is_null() {
            return Err(RunError::NoName);
        }
        let name = crate::binding::Binding::read(binding).name;
        if name.is_null() {
            return Err(RunError::NoName);
        }
        Ok(name as i64)
    }
}

/// The dyad a `[tape, k, i, …]` read names: the cell, or its line `i` when `i` is not null.
///
/// # Safety
/// `ops` must be the operands of a read built by `build_cell_type` or its siblings.
pub(crate) unsafe fn read_target(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<DyadPtr, RunError> {
    if (*ops.add(2)).is_null() {
        return match slot_cell(rt, ops)? {
            Some((_, _, cell)) => Ok(cell),
            None => Err(RunError::OffTape),
        };
    }
    let lines = cell_lines(rt, ops)?;
    let i = rt.run(*ops.add(2))?;
    if i < 0 {
        return Err(RunError::BadIndex(i));
    }
    match lines.get(i as usize) {
        Some(&line) => Ok(rt.through(line)),
        None => Err(RunError::PastEnd { index: i, size: lines.len() }),
    }
}

/// An expression's type is the type of what it yields, a name's its declared type; a bracket
/// is a `scope` or `square_brackets`, whose lines the constructor reads.
fn run_cell_type(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let target = read_target(rt, (*node).value as *const DyadPtr)?;
        let types = rt.types();
        if (*target).ty == types.scope || (*target).ty == types.square_brackets {
            return Ok((*target).ty as i64);
        }
        if crate::parse::is_bool_result(types, target) {
            return Ok(types.bool_ as i64);
        }
        Ok(match numtype_of(types, target) {
            Operand::Concrete(nt) => types.numtypes[nt as usize],
            _ => (*target).ty,
        } as i64)
    }
}

/// Checked again here: a tape edited between the check and this read may hold another cell.
fn run_cell_value(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = read_target(rt, (*node).value as *const DyadPtr)?;
        if (*cell).ty == rt.types().type_ {
            rt.run(cell)
        } else {
            Err(RunError::CellNotAType)
        }
    }
}

/// The number a literal or a constant of the type holds; any other dyad, an expression
/// whose value is known only when it runs, is refused.
///
/// # Safety
/// `cell` must be a dyad from the store; `ty` a number type node.
pub(crate) unsafe fn constant_number(types: &Core, cell: DyadPtr, ty: DyadPtr) -> Option<i64> {
    let nt = super::numtype::of_type_node(ty);
    if (*cell).ty == types.rational {
        return super::rational::mold_to(cell, nt);
    }
    let v = (*cell).value;
    ((*cell).ty == ty && !v.is_null() && !crate::dyad::is_place(v))
        .then(|| super::numtype::read_scalar(ty, v as *const u8))
}

/// Checked again here, as `run_cell_value` is.
fn run_cell_number(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let cell = read_target(rt, ops)?;
        let ty = *ops.add(3);
        constant_number(rt.types(), cell, ty).ok_or(RunError::CellNotANumber(ty))
    }
}

/// The scope or `square_brackets` the cell holds; both keep their `dyads` first.
unsafe fn scope_cell(rt: &mut Runtime, ops: *const DyadPtr) -> Result<DyadPtr, RunError> {
    let Some((_, _, cell)) = slot_cell(rt, ops)? else {
        return Err(RunError::OffTape);
    };
    let types = rt.types();
    if (*cell).ty != types.scope && (*cell).ty != types.square_brackets {
        return Err(RunError::NotAScope(cell));
    }
    Ok(cell)
}

/// A scope that never held a line has none.
unsafe fn cell_lines<'a>(rt: &mut Runtime, ops: *const DyadPtr) -> Result<&'a [DyadPtr], RunError> {
    Ok(super::scope::exprs_of(scope_cell(rt, ops)?).unwrap_or(&[]))
}

fn run_cell_dyads(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = scope_cell(rt, (*node).value as *const DyadPtr)?;
        let array_ty = rt.types().array_;
        Ok(super::scope::dyads(rt.store(), array_ty, cell) as i64)
    }
}

fn run_cell_dyads_size(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe { Ok(cell_lines(rt, (*node).value as *const DyadPtr)?.len() as i64) }
}

/// The dyad a tape read names, a cell or a line of one; `None` for any other node.
///
/// # Safety
/// `read` must be a reduced dyad from the store.
unsafe fn named_dyad(rt: &mut Runtime, read: DyadPtr) -> Option<Result<DyadPtr, RunError>> {
    let ids = rt.types().tape;
    let ty = (*read).ty;
    if ty == ids.slot || ty == ids.cell_dyad_at {
        return Some(rt.run(read).map(|d| rt.through(d as DyadPtr)));
    }
    if ty == ids.cell_value || ty == ids.cell_number {
        return Some(read_target(rt, (*read).value as *const DyadPtr));
    }
    None
}

/// A literal molds to the parameter's type; any other operand must already be of it.
///
/// # Safety
/// `operand` must be a dyad from the store; `ty` a number type node.
unsafe fn typed_operand(
    rt: &mut Runtime,
    operand: DyadPtr,
    ty: DyadPtr,
) -> Result<DyadPtr, RunError> {
    let types = rt.types();
    let typed = if let Operand::Literal = numtype_of(types, operand) {
        let types: *const crate::Core = types;
        // SAFETY: the `Core` outlives the runtime that borrowed it.
        super::commit_literal_to(rt.store(), &*types, operand, ty)
    } else {
        super::check_store_type(types, ty, operand).map(|()| operand)
    };
    typed.map_err(|e| RunError::Parse(Box::new(e)))
}

/// An argument read from the tape is the operand it names, typed now and run when the placed
/// call runs; any other argument reads the constructor's `this` or locals, gone by then, so it
/// runs now. DESIGN ›`a[k]` is an application, exactly as `a(k)`‹.
fn run_placed_call(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[call, op]` node `cell_arg` built over a call node.
    unsafe {
        let call = rt.through(*((*node).value as *const DyadPtr));
        let super::read::Read::Executable(super::read::Dispatch::Call(f)) =
            super::read::read_kind(rt.types(), call)
        else {
            return Err(RunError::NoLeaf);
        };
        let input = *((*f).value as *const DyadPtr).add(crate::parse::FN_INPUT);
        let params = super::array::items(meta::record_fields_of(input));
        let written = (*call).value as *const DyadPtr;
        let arg_at =
            |i: usize| if written.is_null() { std::ptr::null_mut() } else { *written.add(i) };
        if params.len() != (0..).take_while(|&i| !arg_at(i).is_null()).count() {
            return Err(RunError::ArityMismatch);
        }
        let mut args = Vec::with_capacity(params.len() + 1);
        for (i, &param) in params.iter().enumerate() {
            let ty = (*param).ty;
            let scalar = super::numtype::is_scalar_type(ty);
            if let Some(operand) = named_dyad(rt, arg_at(i)) {
                let operand = operand?;
                args.push(if scalar { typed_operand(rt, operand, ty)? } else { operand });
                continue;
            }
            let bits = rt.run(arg_at(i))?;
            let dyad_ty = rt.types().dyad_;
            let store = rt.store();
            args.push(if scalar {
                let width = super::numtype::of_type_node(ty).bytes();
                let storage = store.alloc_bytes(&bits.to_ne_bytes()[..width]);
                store.alloc_raw(ty, storage)
            } else {
                store.alloc_raw(dyad_ty, bits as usize as *mut u8)
            });
        }
        let store = rt.store();
        let value = if args.is_empty() {
            std::ptr::null_mut()
        } else {
            args.push(std::ptr::null_mut());
            store.alloc_operands(&args)
        };
        Ok(store.alloc_raw(f, value) as i64)
    }
}

fn run_cell_dyad_at(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let lines = cell_lines(rt, ops)?;
        let i = rt.run(*ops.add(2))?;
        if i < 0 {
            return Err(RunError::BadIndex(i));
        }
        match lines.get(i as usize) {
            Some(&line) => Ok(line as i64),
            None => Err(RunError::PastEnd { index: i, size: lines.len() }),
        }
    }
}
