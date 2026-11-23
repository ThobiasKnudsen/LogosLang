// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `parsing_tape` and the tape's affordances as identities: `t[k]` read and write,
//! `t.is_constructed[k]`, `t.spelling[k]`, `t.insert`, `t.remove`, `t.recenter`, `t.construct`, and
//! the cell reads `t[k]:name`, `t[k].type`, `t[k].gives`, a `t[k]` checked to hold a type as
//! that type or checked against a number type as that number, and a scope or `[…]` cell's
//! `t[k].dyads`, `t[k].dyads.size` and `t[k].dyads[i]`, whose line reads as a cell does. The
//! type's one field, `cells`, is the seed's `ParsingTape` handle. The natives run interpreted;
//! only the bracket a placed call is handed lowers.

use super::callable::{self, Callables};
use super::numtype::NumType;
use super::place::{At, Placed};
use super::pointer::at_dyad;
use super::read::Carry;
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Cell, ParseError, ParsingTape, TapeRead};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;
use cranelift_codegen::ir::Value;

/// The handles: the type, and each native's identity with its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct TapeIds {
    pub parsing_tape: DyadPtr,
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    /// `tape[0].type = T` for a `T` the parse gets as it runs: `[tape, k, type, op]`, a new
    /// node of that type placed in the cell, the node the parse builds from there.
    pub stamp: DyadPtr,
    pub stamp_leaf: DyadPtr,
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
    /// `t.construct(from, to)`: `[tape, from, to, op]`, the nodes from `from` up to `to` built
    /// as one line.
    pub construct: DyadPtr,
    pub construct_leaf: DyadPtr,
    /// `t[k]:name`: the spelling of the binding the cell holds, a string node.
    pub slot_name: DyadPtr,
    pub slot_name_leaf: DyadPtr,
    /// `t[k].type`: the type of the dyad the cell names, read; `[tape, k, i]`, `i` null
    /// for the cell itself or the line `t[k].dyads[i]`, as in the two reads below.
    pub cell_type: DyadPtr,
    pub cell_type_leaf: DyadPtr,
    /// `t[k].gives`: the type the dyad the cell names gives when it runs; operands as
    /// `cell_type`'s.
    pub cell_gives: DyadPtr,
    pub cell_gives_leaf: DyadPtr,
    /// `t[k]` after a check that the cell holds a type: the type it holds.
    pub cell_value: DyadPtr,
    pub cell_value_leaf: DyadPtr,
    /// `t[k]` after a check against a number type: `[tape, k, i, type]`, the number it holds.
    pub cell_number: DyadPtr,
    pub cell_number_leaf: DyadPtr,
    /// `t[k]` after a check against a type a Logos `parse` builds: `[tape, k, i, type]`, the
    /// address of the node it holds.
    pub cell_node: DyadPtr,
    pub cell_node_leaf: DyadPtr,
    /// `t[k].dyads`: the scope cell's `dyads` array, as its address.
    pub cell_dyads: DyadPtr,
    pub cell_dyads_leaf: DyadPtr,
    /// `t[k].dyads.size`, a `u64`.
    pub cell_dyads_size: DyadPtr,
    pub cell_dyads_size_leaf: DyadPtr,
    /// `t[k].dyads[i]`: the scope cell's line i, as its address.
    pub cell_dyad_at: DyadPtr,
    pub cell_dyad_at_leaf: DyadPtr,
    /// `[bracket, op]`: a bracket handed to a placed call, its lines evaluated when it runs.
    pub bracket_arg: DyadPtr,
    pub bracket_arg_leaf: DyadPtr,
    /// What a read gets where the tape reaches no cell: a `void` node, already constructed.
    pub nothing: DyadPtr,
    /// `[tape, k, i, type, op]`: a line stored into a place of `type`, built by that type's
    /// parse when it is a bracket (DESIGN ›A type body describes one level‹).
    pub cell_into: DyadPtr,
    pub cell_into_leaf: DyadPtr,
    /// `[tape, k, op]`: the identity the cell names, read through its binding.
    pub cell_identity: DyadPtr,
    pub cell_identity_leaf: DyadPtr,
}

/// The members are declared in the type's own scope, where `.` resolves them, each `pub`, since
/// every parse reads them from outside; the field list holds only `cells`, so an instance is
/// eight bytes.
pub(super) fn register(
    cx: &mut Cx,
    cs: &Callables,
    scope_ty: DyadPtr,
    array_ty: DyadPtr,
    void_ty: DyadPtr,
    pub_mint: DyadPtr,
) -> TapeIds {
    let scope = super::scope::mint(cx.store, scope_ty, std::ptr::null_mut());
    // SAFETY: `void_ty` is the type node `Core::build` minted.
    let at_void = unsafe { super::pointer::early_pointer_type(cx, void_ty) };
    let cells = super::hole::build(cx.store, cx.unknown, at_void, std::ptr::null_mut());
    let binding = cx.declare_in(scope, "cells", cells);
    let fields = super::array::build(cx.store, array_ty, &[cells]);
    let bindings = super::array::build(cx.store, array_ty, &[binding]);
    let layout = meta::record_layout(
        cx.store,
        scope,
        fields,
        bindings,
        meta::Extent { size_bytes: 8, align: 8 },
        std::ptr::null_mut(),
        meta::prec::APPLY,
        crate::parse::Assoc::Left,
        cx.derived_parse,
    );
    let parsing_tape = cx.store.alloc_head(cx.type_, layout);
    cx.declare("parsing_tape", parsing_tape);

    let op = |cx: &mut Cx, roles: &[&str], run: crate::run::RunFn| {
        let record = meta::operand_record(
            cx,
            meta::TUPLE_TAG,
            meta::prec::INERT,
            crate::parse::Assoc::Left,
            roles,
        );
        let id = cx.store.alloc_head(cx.type_, record);
        let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);
        (id, leaf)
    };
    let read = |cx: &mut Cx, roles: &[&str], run: crate::run::RunFn| {
        let roles: Vec<&str> = roles.iter().copied().chain(["op", "output_type"]).collect();
        op(cx, &roles, run)
    };
    let (slot, slot_leaf) = read(cx, &["tape", "k"], run_slot);
    let (write, write_leaf) = op(cx, &["tape", "k", "cell", "op"], run_write);
    let (stamp, stamp_leaf) = op(cx, &["tape", "k", "type", "op"], run_stamp);
    let (is_constructed, is_constructed_leaf) = read(cx, &["tape", "k"], run_is_constructed);
    let (flag_write, flag_write_leaf) = op(cx, &["tape", "k", "flag", "op"], run_flag_write);
    let (spelling, spelling_leaf) = read(cx, &["tape", "k"], run_spelling);
    let (insert, insert_leaf) = op(cx, &["tape", "k", "cells", "op"], run_insert);
    let (remove, remove_leaf) = op(cx, &["tape", "k", "op"], run_remove);
    let (recenter, recenter_leaf) = op(cx, &["tape", "k", "op"], run_recenter);
    let (construct, construct_leaf) = op(cx, &["tape", "k", "to", "op"], run_construct);
    let (slot_name, slot_name_leaf) = read(cx, &["tape", "k"], run_slot_name);
    let (cell_type, cell_type_leaf) = read(cx, &["tape", "k", "i"], run_line_type);
    let (cell_gives, cell_gives_leaf) = read(cx, &["tape", "k", "i"], run_line_type);
    let (cell_value, cell_value_leaf) = read(cx, &["tape", "k", "i"], run_cell_value);
    let (cell_number, cell_number_leaf) = read(cx, &["tape", "k", "i", "type"], run_cell_number);
    let (cell_node, cell_node_leaf) = read(cx, &["tape", "k", "i", "type"], run_cell_node);
    let (cell_dyads, cell_dyads_leaf) = read(cx, &["tape", "k"], run_cell_dyads);
    let (cell_dyads_size, cell_dyads_size_leaf) = read(cx, &["tape", "k"], run_cell_dyads_size);
    let (cell_dyad_at, cell_dyad_at_leaf) = read(cx, &["tape", "k", "i"], run_cell_dyad_at);
    let (bracket_arg, bracket_arg_leaf) = read(cx, &["bracket"], run_bracket_arg);
    let (cell_into, cell_into_leaf) = read(cx, &["tape", "k", "i", "type"], run_cell_into);
    let (cell_identity, cell_identity_leaf) = read(cx, &["tape", "k"], run_cell_identity);
    cx.lower.insert(bracket_arg, lower_bracket_arg);
    for id in [slot, cell_dyad_at, cell_value, cell_number, cell_node, cell_into] {
        cx.placings.insert(id, place_line);
    }
    for id in [cell_type, cell_gives] {
        cx.placings.insert(id, super::place::run_now);
    }
    cx.placings.insert(bracket_arg, super::place::whole);
    let nothing = cx.store.alloc_leaf(void_ty);
    for (name, id) in [
        ("is_constructed", is_constructed),
        ("spelling", spelling),
        ("insert", insert),
        ("remove", remove),
        ("recenter", recenter),
        ("construct", construct),
    ] {
        // SAFETY: `pub_mint` is a gate mint and `id` an identity just built.
        let gated = unsafe { super::gate::over_node(cx.store, pub_mint, id) };
        cx.declare_in(scope, name, gated);
    }
    TapeIds {
        parsing_tape,
        slot,
        slot_leaf,
        write,
        write_leaf,
        stamp,
        stamp_leaf,
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
        construct,
        construct_leaf,
        slot_name,
        slot_name_leaf,
        cell_type,
        cell_type_leaf,
        cell_gives,
        cell_gives_leaf,
        cell_value,
        cell_value_leaf,
        cell_number,
        cell_number_leaf,
        cell_node,
        cell_node_leaf,
        cell_dyads,
        cell_dyads_leaf,
        cell_dyads_size,
        cell_dyads_size_leaf,
        cell_dyad_at,
        cell_dyad_at_leaf,
        bracket_arg,
        bracket_arg_leaf,
        nothing,
        cell_into,
        cell_into_leaf,
        cell_identity,
        cell_identity_leaf,
    }
}

pub(crate) fn member(ids: &TapeIds, name: &str) -> Option<(DyadPtr, DyadPtr)> {
    match name {
        "is_constructed" => Some((ids.is_constructed, ids.is_constructed_leaf)),
        "spelling" => Some((ids.spelling, ids.spelling_leaf)),
        "insert" => Some((ids.insert, ids.insert_leaf)),
        "remove" => Some((ids.remove, ids.remove_leaf)),
        "recenter" => Some((ids.recenter, ids.recenter_leaf)),
        "construct" => Some((ids.construct, ids.construct_leaf)),
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
    if dyad::ty(lhs) == types.deref_ {
        let (ptr_expr, pointee, off) = super::pointer::deref_parts(lhs);
        if pointee == types.tape.parsing_tape && off == 0 {
            return Some(ptr_expr);
        }
        return None;
    }
    if types.type_of(lhs) == types.tape.parsing_tape
        && (types.is_storage(lhs) || !types.is_hole(lhs))
    {
        return Some(super::pointer::build_addr(store, types, lhs, std::ptr::null_mut()));
    }
    None
}

fn node(
    store: &mut Store,
    op: DyadPtr,
    leaf: DyadPtr,
    operands: &[DyadPtr],
    output: DyadPtr,
) -> DyadPtr {
    let mut v = operands.to_vec();
    v.extend([leaf, output]);
    store.alloc_words(op, &v)
}

/// A write to the tape gives its line nothing, so its node has no output word.
fn act(store: &mut Store, op: DyadPtr, leaf: DyadPtr, operands: &[DyadPtr]) -> DyadPtr {
    let mut v = operands.to_vec();
    v.push(leaf);
    store.alloc_words(op, &v)
}

/// A plain number written as an index lands in `i64`, as a plain number lands in any number type
/// (›The tape is a cheap view with a movable center, over a doubly linked list‹ [[R250]]); any
/// other index stands as written.
///
/// # Safety
/// `k` must be null or a reduced dyad from the store.
unsafe fn index(store: &mut Store, types: &Core, k: DyadPtr) -> Result<DyadPtr, ParseError> {
    if k.is_null() {
        return Ok(k);
    }
    super::land_rational(store, types, k, types.numtypes[NumType::I64 as usize])
}

/// `t[k]`: read, the cell's pointer; as `=`'s target, the write (`build_write`).
///
/// # Safety
/// `recv` and `k` must be reduced dyads from the store.
pub(crate) unsafe fn build_slot(
    store: &mut Store,
    types: &Core,
    recv: DyadPtr,
    k: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let k = index(store, types, k)?;
    let output = at_dyad(store, types);
    Ok(node(store, types.tape.slot, types.tape.slot_leaf, &[recv, k], output))
}

/// The cell takes the node placing makes of `cell` when the write runs.
///
/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_write(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    cell: DyadPtr,
) -> DyadPtr {
    let ops = dyad::value(slot) as *const DyadPtr;
    let (recv, k) = (*ops, *ops.add(1));
    let cell = super::place::build(store, types, cell);
    act(store, types.tape.write, types.tape.write_leaf, &[recv, k, cell])
}

/// The cell takes a new node of the type `ty` gives when the stamp runs.
///
/// # Safety
/// `slot` must be a slot node from `build_slot`; `ty` a reduced dyad that gives a type.
pub(crate) unsafe fn build_stamp(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    act(store, types.tape.stamp, types.tape.stamp_leaf, &[recv, k, ty])
}

/// # Safety
/// `over` must be a slot node or a node built from one here.
unsafe fn slot_parts(over: DyadPtr) -> (DyadPtr, DyadPtr) {
    let ops = dyad::value(over) as *const DyadPtr;
    (*ops, *ops.add(1))
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_slot_name(store: &mut Store, types: &Core, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    // ›`x:name` is the name's spelling, held by the binding‹ [[R285]]
    node(store, types.tape.slot_name, types.tape.slot_name_leaf, &[recv, k], types.string_)
}

/// The tape, the cell index, and the line index of a line read, null for the cell itself.
///
/// # Safety
/// `read` must be a slot node, a line node from `build_cell_dyad_at`, or a node built
/// from either by the builders below.
unsafe fn read_parts(types: &Core, read: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let (recv, k) = slot_parts(read);
    let ty = dyad::ty(read);
    let has_line = ty == types.tape.cell_dyad_at
        || ty == types.tape.cell_type
        || ty == types.tape.cell_gives
        || ty == types.tape.cell_value
        || ty == types.tape.cell_number
        || ty == types.tape.cell_node
        || ty == types.tape.cell_into;
    let i =
        if has_line { *(dyad::value(read) as *const DyadPtr).add(2) } else { std::ptr::null_mut() };
    (recv, k, i)
}

/// Which type a read of a node's type gives: `.type`, the node's own, or `.gives`, the type the
/// node gives when it runs (›`.gives` reads the type a node gives when it runs, on any node;
/// `.type` stays the node's own type‹ [[R387]]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRead {
    Own,
    Gives,
}

impl TypeRead {
    /// The member that reads it, `type` or `gives`; `None` for any other.
    pub(crate) fn of_member(name: &str) -> Option<Self> {
        match name {
            "type" => Some(Self::Own),
            "gives" => Some(Self::Gives),
            _ => None,
        }
    }
}

/// `t[k].type` or `t[k].dyads[i].type`, or `.gives` of either.
///
/// # Safety
/// As `read_parts`.
pub(crate) unsafe fn build_cell_type(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    which: TypeRead,
) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    let ids = &types.tape;
    let (op, leaf) = match which {
        TypeRead::Own => (ids.cell_type, ids.cell_type_leaf),
        TypeRead::Gives => (ids.cell_gives, ids.cell_gives_leaf),
    };
    node(store, op, leaf, &[recv, k, i], types.type_)
}

/// DESIGN ›A type is a comptime value, resolved in the pass‹, a checked tape cell.
///
/// # Safety
/// As `read_parts`.
pub(crate) unsafe fn build_cell_value(store: &mut Store, types: &Core, read: DyadPtr) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    node(store, types.tape.cell_value, types.tape.cell_value_leaf, &[recv, k, i], types.type_)
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
    node(store, types.tape.cell_number, types.tape.cell_number_leaf, &[recv, k, i, ty], ty)
}

/// A read of the cell itself, plain or checked: what `=`, `:` and `.dyads` take.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_cell_read(types: &Core, node: DyadPtr) -> bool {
    dyad::ty(node) == types.tape.slot || dyad::ty(node) == types.tape.cell_value
}

/// A read a `.` may follow: of a node the tape holds, or of a type that node is or gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeRead {
    /// A cell or a line of one, `tape[k]` or `tape[k].dyads[i]`; `own` where it is the cell the
    /// parse was woken on, which holds the node the parse builds.
    Node { own: bool },
    /// `.type` or `.gives` of one.
    Type,
}

/// Which read of a tape node `node` is, a cell or line plain or checked to hold a type, or the
/// `.type` or `.gives` of one, with `own` asked for a cell; `None` for any other node.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn node_read(
    types: &Core,
    node: DyadPtr,
    own: impl FnOnce() -> bool,
) -> Option<NodeRead> {
    if is_cell_read(types, node) {
        return Some(NodeRead::Node { own: own() });
    }
    let ty = dyad::ty(node);
    if ty == types.tape.cell_dyad_at {
        return Some(NodeRead::Node { own: false });
    }
    (ty == types.tape.cell_type || ty == types.tape.cell_gives).then_some(NodeRead::Type)
}

/// What answers `.name` after a read of a tape node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeMember {
    /// A type the read is or gives, `.type` or `.gives`, read when the parse runs.
    Type,
    /// A bracket's lines.
    Lines,
    /// A field of the node the parse builds, reached through its own cell.
    Own,
    /// A member of what the tape holds when the parse runs, read through its type: the node's,
    /// or after `.type` its type's. Which node that is is known only then, so the line reading
    /// it waits (›`x.type` reads the type word of the node a name or an expression stands for‹
    /// [[R279]], ›A type known only when the program runs still works; what needs it is parsed
    /// when its line runs‹ [[R377]]).
    Held,
}

/// The one decision the line scan and the member read share: what answers `.name` after `read`.
pub(crate) fn node_member(read: NodeRead, name: &str) -> NodeMember {
    match (name, read) {
        _ if TypeRead::of_member(name).is_some() => NodeMember::Type,
        (_, NodeRead::Type) => NodeMember::Held,
        ("dyads", _) => NodeMember::Lines,
        (_, NodeRead::Node { own: true }) => NodeMember::Own,
        _ => NodeMember::Held,
    }
}

/// What `read`, a read [`node_read`] knows, gives now: the node the tape holds, a name read
/// through to the dyad it names, or the type that node is or gives.
///
/// # Safety
/// `read` must be a node [`node_read`] knows; the constructor it was written in must be running.
pub(crate) unsafe fn held_now(rt: &mut Runtime, read: DyadPtr) -> Result<DyadPtr, RunError> {
    if type_read_of(rt.types(), read).is_some() {
        return run_line_type(rt, read).map(|t| t as DyadPtr);
    }
    named_line(rt, read).expect("a read of a tape node names one")
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
    let recv = super::pointer::address_place(types, recv).unwrap_or_else(|| types.through(recv));
    if k.is_null() {
        return Some((recv, -1, line_of(types, i)?));
    }
    let k = constant_of(types, types.through(k), types.numtypes[NumType::I64 as usize])?;
    Some((recv, i32::try_from(k).ok()?, line_of(types, i)?))
}

/// # Safety
/// `i` must be null or a reduced dyad from the store.
unsafe fn line_of(types: &Core, i: DyadPtr) -> Option<Line> {
    if i.is_null() {
        return Some(Line::Cell);
    }
    let i = types.through(i);
    Some(match constant_of(types, i, types.numtypes[NumType::I64 as usize]) {
        Some(at) => Line::At(i32::try_from(at).ok()?),
        None => Line::Named(i),
    })
}

/// `b.dyads` of a bracket `b` a call was handed: a cell read with no cell index.
pub(crate) fn build_bracket_dyads(store: &mut Store, types: &Core, held: DyadPtr) -> DyadPtr {
    let output = at_dyad(store, types);
    let ops = [held, std::ptr::null_mut()];
    node(store, types.tape.cell_dyads, types.tape.cell_dyads_leaf, &ops, output)
}

/// `b.dyads[i]` of a bracket `b` a call was handed.
///
/// # Safety
/// `i` must be a reduced dyad from the store.
pub(crate) unsafe fn build_bracket_line(
    store: &mut Store,
    types: &Core,
    held: DyadPtr,
    i: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let i = index(store, types, i)?;
    let output = at_dyad(store, types);
    let ops = [held, std::ptr::null_mut(), i];
    Ok(node(store, types.tape.cell_dyad_at, types.tape.cell_dyad_at_leaf, &ops, output))
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_cell_dyads(store: &mut Store, types: &Core, slot: DyadPtr) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    let output = at_dyad(store, types);
    node(store, types.tape.cell_dyads, types.tape.cell_dyads_leaf, &[recv, k], output)
}

/// # Safety
/// `slot` must be a slot node from `build_slot`.
pub(crate) unsafe fn build_cell_identity(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(slot);
    node(store, types.tape.cell_identity, types.tape.cell_identity_leaf, &[recv, k], types.type_)
}

fn run_cell_identity(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = read_cell(rt, dyad::value(node) as *const DyadPtr)?;
        Ok(rt.through(cell.dyad) as i64)
    }
}

/// # Safety
/// `dyads` must be a node from `build_cell_dyads`.
pub(crate) unsafe fn build_cell_dyads_size(
    store: &mut Store,
    types: &Core,
    dyads: DyadPtr,
) -> DyadPtr {
    let (recv, k) = slot_parts(dyads);
    let output = types.numtypes[NumType::U64 as usize];
    node(store, types.tape.cell_dyads_size, types.tape.cell_dyads_size_leaf, &[recv, k], output)
}

/// # Safety
/// `slot` must be a slot node from `build_slot`; `i` a reduced dyad.
pub(crate) unsafe fn build_cell_dyad_at(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    i: DyadPtr,
) -> Result<DyadPtr, ParseError> {
    let (recv, k) = slot_parts(slot);
    let i = index(store, types, i)?;
    let output = at_dyad(store, types);
    Ok(node(store, types.tape.cell_dyad_at, types.tape.cell_dyad_at_leaf, &[recv, k, i], output))
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
    act(store, types.tape.flag_write, types.tape.flag_write_leaf, &[recv, k, flag])
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
        let k = index(store, types, k)?;
        let cells = if super::lex::is_fragment(types, cells) {
            cells
        } else {
            let d = types.through(cells);
            receiver_addr(store, types, d).ok_or(ParseError::InsertTakesTape)?
        };
        return Ok(act(store, op, leaf, &[recv, k, cells]));
    }
    if op == ids.construct {
        let [from, to] = args[..] else {
            return Err(ParseError::CtorArity);
        };
        let (from, to) = (index(store, types, from)?, index(store, types, to)?);
        return Ok(act(store, op, leaf, &[recv, from, to]));
    }
    let [k] = args[..] else {
        return Err(ParseError::CtorArity);
    };
    let k = index(store, types, k)?;
    if op == ids.recenter || op == ids.remove {
        return Ok(act(store, op, leaf, &[recv, k]));
    }
    // ›`tape.spelling[k]` is the text a cell was lexed from‹ [[R249]]
    let output = if op == ids.is_constructed {
        types.bool_
    } else if op == ids.spelling {
        types.string_
    } else {
        at_dyad(store, types)
    };
    Ok(node(store, op, leaf, &[recv, k], output))
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

/// The cell a read names; past the end of the source, a boundary, the left edge or a node being
/// built, the constructed `void` node (›A `tape[k]` read past the lexed frontier lexes on demand
/// for every constructor‹ [[R231]]).
unsafe fn read_cell(rt: &mut Runtime, ops: *const DyadPtr) -> Result<Cell, RunError> {
    let (tape, k) = tape_at(rt, ops)?;
    Ok(cell_on(rt, tape, k))
}

/// # Safety
/// `tape` must be a live `ParsingTape`.
unsafe fn cell_on(rt: &Runtime, tape: *mut ParsingTape, k: isize) -> Cell {
    let nothing = rt.types().tape.nothing;
    (*tape).in_reach(k).copied().unwrap_or_else(|| Cell::built(nothing))
}

/// A cell of the running constructor's tape is taken as an operation takes its operand
/// (`Parser::read_tape`).
fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let (tape, k) = tape_at(rt, dyad::value(node) as *const DyadPtr)?;
        let taken = rt.read_on_tape(tape, k, TapeRead::Cell)?;
        Ok(taken.unwrap_or_else(|| cell_on(rt, tape, k).dyad) as i64)
    }
}

/// The pointer replaced and nothing more; the flag is the constructor's line.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        let dyad = rt.run(*ops.add(2))? as DyadPtr;
        if !(*tape).set_dyad(k, dyad) {
            return Err(RunError::OffTape);
        }
        rt.stamp(dyad);
        Ok(0)
    }
}

/// Only a type whose values are nodes has slots for the parse to fill; a plain record's value
/// is its bytes.
fn run_stamp(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a stamp node from `build_stamp`; its type operand gives a type node or
    // null, and `is_record_type` admits only a record type to `empty_node`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        let ty = rt.run(*ops.add(2))? as DyadPtr;
        if ty.is_null() || !meta::is_record_type(ty) || super::this::is_plain(rt.types(), ty) {
            return Err(RunError::StampNotANodeType);
        }
        let fresh = super::this::empty_node(rt.store(), ty);
        if !(*tape).set_dyad(k, fresh) {
            return Err(RunError::OffTape);
        }
        rt.restamp(fresh);
        Ok(0)
    }
}

fn run_flag_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
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
    unsafe { Ok(i64::from(read_cell(rt, dyad::value(node) as *const DyadPtr)?.constructed)) }
}

/// A cell nothing lexed answers the empty text. The string node is built into the
/// parser's store, so the read needs one attached.
fn run_spelling(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = read_cell(rt, dyad::value(node) as *const DyadPtr)?;
        let text = cell.spelling();
        let string_ty = rt.types().string_;
        let store = rt.store();
        Ok(super::string::build_text(store, string_ty, text.as_bytes()) as i64)
    }
}

/// The fragment's cells are copied out first, so a tape spliced into itself is well-defined.
fn run_insert(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
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
        let ops = dyad::value(node) as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        if (*tape).in_reach(k).is_none() {
            return Ok(0);
        }
        // A bracket taken off the tape never runs, so its lines are no scope's.
        if let Some(cell) = (*tape).remove(k) {
            rt.note_lines_read(cell.dyad);
        }
        Ok(0)
    }
}

fn run_recenter(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (tape, k) = tape_at(rt, ops)?;
        (*tape).recenter(k);
        Ok(0)
    }
}

/// The tape is lexed on to the node before `to` first, so a `to` past the line's end is off it.
fn run_construct(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (tape, from) = tape_at(rt, ops)?;
        let to = rt.run(*ops.add(2))? as isize;
        if to > from {
            rt.reach(tape, to - 1)?;
            if (*tape).at(to - 1).is_none() {
                return Err(RunError::OffTape);
            }
        }
        rt.construct_on_tape(tape, from, to)?;
        Ok(0)
    }
}

/// The cell, read through a binding to the dyad it names; with no cell index, the bracket
/// the receiver holds, a bracket handed to a call.
unsafe fn slot_cell(rt: &mut Runtime, ops: *const DyadPtr) -> Result<DyadPtr, RunError> {
    if (*ops.add(1)).is_null() {
        let held = rt.run(*ops)? as DyadPtr;
        if held.is_null() {
            return Err(RunError::NotAScope(held));
        }
        return Ok(rt.through(held));
    }
    let cell = read_cell(rt, ops)?;
    Ok(rt.through(cell.dyad))
}

/// The identity's name, never the appearance's text (that is `t.spelling[k]`); a cell
/// holding no binding has no name.
fn run_slot_name(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let c = read_cell(rt, dyad::value(node) as *const DyadPtr)?;
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
        return slot_cell(rt, ops);
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

/// `.type` of a line a parse holds is its node's own type, a bracket's `scope` or
/// `square_brackets`, and `.gives` the type the node gives when it runs (›`.gives` reads the type a
/// node gives when it runs, on any node; `.type` stays the node's own type‹ [[R387]]). A cell of
/// the running constructor's tape is read as an operation reads its operand (`Parser::read_tape`).
pub(crate) fn run_line_type(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a node `is_line_type_read` accepts.
    unsafe {
        let which = type_read_of(rt.types(), node).expect("a read of a line's type");
        let line = if is_cell_type_read(rt.types(), node) {
            let (tape, k) = tape_at(rt, dyad::value(node) as *const DyadPtr)?;
            let read = match which {
                TypeRead::Own => TapeRead::Type,
                TypeRead::Gives => TapeRead::Gives,
            };
            match rt.read_on_tape(tape, k, read)? {
                Some(operand) => operand,
                None => rt.through(cell_on(rt, tape, k).dyad),
            }
        } else {
            line_of_type_read(rt, node)?
        };
        let types = rt.types();
        let ty = match which {
            TypeRead::Own => super::read::own_type(types, line),
            TypeRead::Gives => {
                super::read::gives(types, line).map_err(|e| RunError::Parse(Box::new(e)))?
            }
        };
        Ok(ty as i64)
    }
}

/// A read of a tape cell itself, no line of it.
///
/// # Safety
/// `node` must be a node `is_line_type_read` accepts.
unsafe fn is_cell_type_read(types: &Core, node: DyadPtr) -> bool {
    let ops = dyad::value(node) as *const DyadPtr;
    !is_field_type_read(types, node) && !(*ops.add(1)).is_null() && (*ops.add(2)).is_null()
}

/// Which type `node` reads, where it is `.type` or `.gives` of a line a parse holds: a tape cell,
/// a line of one, or a `dyad` field of the parse's own node; `None` for any other node.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn type_read_of(types: &Core, node: DyadPtr) -> Option<TypeRead> {
    let ty = dyad::ty(node);
    if ty == types.tape.cell_type || ty == types.this.field_type {
        Some(TypeRead::Own)
    } else if ty == types.tape.cell_gives || ty == types.this.field_gives {
        Some(TypeRead::Gives)
    } else {
        None
    }
}

/// `.type` or `.gives` of a line a parse holds ([`type_read_of`]).
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_line_type_read(types: &Core, node: DyadPtr) -> bool {
    type_read_of(types, node).is_some()
}

/// # Safety
/// `read` must be a node `is_line_type_read` accepts.
unsafe fn is_field_type_read(types: &Core, read: DyadPtr) -> bool {
    dyad::ty(read) == types.this.field_type || dyad::ty(read) == types.this.field_gives
}

/// The line a `.type` or `.gives` read reads the type of.
///
/// # Safety
/// `read` must be a node `is_line_type_read` accepts.
pub(crate) unsafe fn line_of_type_read(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<DyadPtr, RunError> {
    if is_field_type_read(rt.types(), read) {
        return super::this::held(rt, *(dyad::value(read) as *const DyadPtr));
    }
    read_target(rt, dyad::value(read) as *const DyadPtr)
}

/// The cell, line or field a `.type` or `.gives` read names, as a check narrows it.
///
/// # Safety
/// `read` must be a node `is_line_type_read` accepts.
pub(crate) unsafe fn type_read_key(types: &Core, read: DyadPtr) -> Option<(DyadPtr, i32, Line)> {
    if is_field_type_read(types, read) {
        return super::this::field_key(types, *(dyad::value(read) as *const DyadPtr));
    }
    cell_key(types, read)
}

/// Checked again here: a tape edited between the check and this read may hold another cell.
fn run_cell_value(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = read_target(rt, dyad::value(node) as *const DyadPtr)?;
        // A gated type, `mut i32`, is a type (›Gates are type nodes: `x := mut i32 5`; the record
        // holds no gates‹ [[R381]]).
        if dyad::ty(super::gate::under(cell)) == rt.types().type_ {
            rt.run(cell)
        } else {
            Err(RunError::CellNotAType)
        }
    }
}

/// The value a constant of `ty` the cell holds is, as a `ty`: the number a literal or a constant
/// of a type lying inside `ty` holds, or for `type` the type the cell names. Any other dyad, an
/// expression whose value is known only when it runs, is refused.
///
/// # Safety
/// `cell` must be a dyad from the store; `ty` `type`, a number type, `bool` or a pointer type node.
pub(crate) unsafe fn constant_value(
    types: &Core,
    cell: DyadPtr,
    ty: DyadPtr,
) -> Result<i64, RunError> {
    constant_of(types, cell, ty)
        .ok_or_else(|| RunError::NotAConstant(Box::new(super::message_type_name(types, ty))))
}

/// # Safety
/// As [`constant_value`].
unsafe fn constant_of(types: &Core, cell: DyadPtr, ty: DyadPtr) -> Option<i64> {
    use super::numtype::{apply_cast, of_type_node, read_scalar};
    use super::read::{place_layout, Read};
    if ty == types.type_ {
        return super::type_identity_of(types, cell).map(|t| t as i64);
    }
    if !matches!(place_layout(types, ty), Some((Read::Scalar(_) | Read::Pointer(_), _))) {
        return None;
    }
    let nt = of_type_node(ty);
    let from = dyad::ty(cell);
    if from == types.rational {
        // `bool` and a pointer share a number's machine form, but no plain number lies inside them.
        if !super::is_numtype_node(types, ty) {
            return None;
        }
        return super::rational::mold_to(cell, nt);
    }
    let v = dyad::value(cell);
    if v.is_null() {
        return None;
    }
    if from == ty {
        return Some(read_scalar(ty, v as *const u8));
    }
    let inside = super::is_numtype_node(types, from)
        && super::subset::includes(types, from, ty) == Some(true);
    if !inside {
        return None;
    }
    apply_cast(of_type_node(from), nt, read_scalar(from, v as *const u8))
}

/// Checked again here, as `run_cell_value` is.
fn run_cell_number(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let cell = read_target(rt, ops)?;
        constant_value(rt.types(), cell, *ops.add(3))
    }
}

/// The same paragraph, a cell or line checked against a type a Logos `parse` builds.
///
/// # Safety
/// As `read_parts`; `ty` such a type.
pub(crate) unsafe fn build_cell_node(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    let (recv, k, i) = read_parts(types, read);
    node(store, types.tape.cell_node, types.tape.cell_node_leaf, &[recv, k, i, ty], ty)
}

/// Checked again here, as `run_cell_value` is.
fn run_cell_node(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let cell = read_target(rt, ops)?;
        let ty = *ops.add(3);
        if super::node_output(rt.types(), cell) != Some(ty) {
            return Err(RunError::CellNotANode);
        }
        rt.run(cell)
    }
}

/// A cell or line no check narrowed, written by `=` into a place of `ty` or passed to a
/// parameter of `ty`, is decided by that type when the write runs: a line that already is a
/// value of `ty` is taken as it is, `type` takes a type, a number type or `bool` the literal or
/// constant the line holds, and a type a Logos `parse` builds builds a bracket by that parse.
/// `None` for any other value, and for a place no whole value moves into or one that holds a
/// node, which [`super::land`] decides. ›A tape read checked against a number type reads as that
/// number‹ [[R229]], ›What `=` does is decided by the type of the place on its left‹ [[R192]],
/// ›An array expression is a node of `array`, and its run makes the value‹ [[R224]].
///
/// # Safety
/// `value` must be a reduced dyad from the store; `ty` null or a type node.
pub(crate) unsafe fn build_cell_into(
    store: &mut Store,
    types: &Core,
    value: DyadPtr,
    ty: DyadPtr,
) -> Option<DyadPtr> {
    use super::read::{carry_of, place_layout, Read};
    let read = types.through(value);
    if dyad::ty(read) != types.tape.cell_dyad_at && dyad::ty(read) != types.tape.slot {
        return None;
    }
    let ty = super::type_identity_of(types, ty)?;
    carry_of(types, ty)?;
    if let Read::Container(t) = place_layout(types, ty)?.0 {
        // A `dyad` place holds the line itself (›Holding is decided at the binding site,
        // parameters included‹ [[R61]]).
        if t != types.type_ && !meta::is_node_valued(t, types.fn_type) {
            return None;
        }
    }
    let (recv, k, i) = read_parts(types, read);
    Some(node(store, types.tape.cell_into, types.tape.cell_into_leaf, &[recv, k, i, ty], ty))
}

/// A bracket is built by `ty`'s parse, then the line must be a value of `ty`.
fn run_cell_into(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let mut line = read_target(rt, ops)?;
        let ty = *ops.add(3);
        if !super::meta::is_node_valued(ty, rt.types().fn_type) {
            if let Some((from, carry)) = made_value(rt.types(), line, ty) {
                let v = super::assign::take(rt, carry, line)?;
                if from == ty {
                    return Ok(v);
                }
                return super::convert::converted(rt.types(), from, ty, v);
            }
            return constant_value(rt.types(), line, ty);
        }
        if is_bracket(rt.types(), line) {
            let built = rt.construct_on_pass(ty, line)? as DyadPtr;
            line = rt.through(built);
        }
        if super::node_output(rt.types(), line) != Some(ty) {
            return Err(RunError::CellNotANode);
        }
        rt.run(line)
    }
}

/// The type of `line` and how it moves, when it is a value the pass has made, a constant or one
/// that ran, of a type that lies inside `ty`: such a line is taken, and converted into `ty` as
/// a conversion left unwritten converts it (›A type's `parse` decides what converts into it;
/// unwritten, a value converts only into a type that holds it exactly‹ [[R155]]). `None` for
/// any other line, a name among them, whose value is not read while the parse runs.
///
/// # Safety
/// `line` must be a reduced dyad from the store; `ty` a type node.
unsafe fn made_value(types: &Core, line: DyadPtr, ty: DyadPtr) -> Option<(DyadPtr, Carry)> {
    let from = super::read::output_type(types, line);
    let made = dyad::ty(line) == from || dyad::ty(line) == types.ran.node;
    let inside = from == ty || super::subset::includes(types, from, ty) == Some(true);
    if !made || !inside {
        return None;
    }
    Some((from, super::read::carry_of(types, from)?))
}

unsafe fn is_bracket(types: &Core, d: DyadPtr) -> bool {
    dyad::ty(d) == types.scope || dyad::ty(d) == types.square_brackets
}

/// The scope or `square_brackets` the cell holds; both keep their `dyads` first.
unsafe fn scope_cell(rt: &mut Runtime, ops: *const DyadPtr) -> Result<DyadPtr, RunError> {
    let cell = slot_cell(rt, ops)?;
    let types = rt.types();
    if dyad::ty(cell) != types.scope && dyad::ty(cell) != types.square_brackets {
        return Err(RunError::NotAScope(cell));
    }
    rt.note_lines_read(cell);
    Ok(cell)
}

/// A scope that never held a line has none.
unsafe fn cell_lines<'a>(rt: &mut Runtime, ops: *const DyadPtr) -> Result<&'a [DyadPtr], RunError> {
    Ok(super::scope::exprs_of(scope_cell(rt, ops)?).unwrap_or(&[]))
}

fn run_cell_dyads(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let cell = scope_cell(rt, dyad::value(node) as *const DyadPtr)?;
        let array_ty = rt.types().array_;
        Ok(super::scope::dyads(rt.store(), array_ty, cell) as i64)
    }
}

fn run_cell_dyads_size(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe { Ok(cell_lines(rt, dyad::value(node) as *const DyadPtr)?.len() as i64) }
}

/// The dyad a tape read names, a cell or a line of one; `None` for any other node.
///
/// # Safety
/// `read` must be a reduced dyad from the store.
pub(crate) unsafe fn named_line(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Option<Result<DyadPtr, RunError>> {
    let ids = rt.types().tape;
    let ty = dyad::ty(read);
    if ty == ids.slot || ty == ids.cell_dyad_at {
        return Some(rt.run(read).map(|d| rt.through(d as DyadPtr)));
    }
    if ty == ids.cell_value || ty == ids.cell_number || ty == ids.cell_node || ty == ids.cell_into {
        return Some(read_target(rt, dyad::value(read) as *const DyadPtr));
    }
    None
}

/// Placed, a read of a cell or line is that line itself, kept as graph for the placed node to run;
/// inside the right side, a line a check narrowed lands in the type it was checked against.
///
/// # Safety
/// `read` must be a reduced dyad from the store, of a read [`named_line`] names a line of.
unsafe fn place_line(rt: &mut Runtime, read: DyadPtr, at: At) -> Result<Placed, RunError> {
    let line = named_line(rt, read).expect("a read placed as a line names one")?;
    Ok(Placed::New(match (at, checked_type(rt.types(), read)) {
        (At::Inside, Some(ty)) => super::place::land(rt, line, ty)?,
        _ => line,
    }))
}

/// The type a read of a cell or line takes its line as; `None` for any other node, and for a type
/// a Logos `parse` builds, whose bracket the call builds.
///
/// # Safety
/// `read` must be a reduced dyad from the store.
pub(crate) unsafe fn checked_type(types: &Core, read: DyadPtr) -> Option<DyadPtr> {
    let ty = dyad::ty(read);
    if ty != types.tape.cell_number && ty != types.tape.cell_into {
        return None;
    }
    let checked = *(dyad::value(read) as *const DyadPtr).add(3);
    (!meta::is_node_valued(checked, types.fn_type)).then_some(checked)
}

/// A bracket a type's `parse` hands to the call it places is the list the new value is built
/// from, so the value owns what its lines hold: a line naming a value whose type fills a `free`
/// puts a copy of it in (›A value owns what its elements hold and frees it‹ [[R226]]), in a new
/// bracket of the same kind; `bracket` itself where no line takes a copy.
///
/// # Safety
/// `bracket` must be a scope or `square_brackets` node from the store.
unsafe fn owned_lines(store: &mut Store, types: &Core, bracket: DyadPtr) -> DyadPtr {
    let lines = super::scope::exprs_of(bracket).unwrap_or(&[]);
    let taken: Vec<DyadPtr> =
        lines.iter().map(|&line| super::copy::taken(store, types, line)).collect();
    if taken == lines {
        return bracket;
    }
    let mut slots = [std::ptr::null_mut(); super::scope::SLOTS];
    slots[0] = super::array::build(store, types.array_, &taken);
    store.alloc_words(dyad::ty(bracket), &slots)
}

/// The operand a node's address gives a placed node: for a type's own fresh node, a copy
/// made each time the placed node runs; for a name or a call yielding a node of a type a Logos
/// `parse` builds, that expression, run when the placed node runs, a call that ends in a
/// dereference as that place (›`a[k]` is an application, exactly as `a(k)`‹ [[R227]]); any other
/// node by a `dyad` view. DESIGN ›A name of a type with an instances' `parse` wakes it, as the
/// instance does‹.
///
/// # Safety
/// `d` must be what a `dyad`-valued operand yielded: null or a node's address.
pub(crate) unsafe fn node_operand(rt: &mut Runtime, d: DyadPtr) -> DyadPtr {
    let types: *const Core = rt.types();
    if rt.fresh_this() == Some(d) {
        return super::this::build_copy(rt.store(), &*types, d);
    }
    let store = rt.store();
    if !d.is_null()
        && super::read::read_kind(&*types, d) != super::read::Read::Node
        && super::node_output(&*types, d).is_some()
    {
        return super::read::on_path(store, &*types, d);
    }
    store.alloc_head((*types).dyad_, d.cast())
}

/// A bracket a node holds in a field: its lines are evaluated each time the node runs, in the
/// frame it runs in, and the run reads a new bracket of their values.
///
/// # Safety
/// `bracket` must be a scope or `square_brackets` node from the store.
pub(crate) unsafe fn build_bracket_arg(
    store: &mut Store,
    types: &Core,
    bracket: DyadPtr,
) -> DyadPtr {
    let bracket = owned_lines(store, types, bracket);
    let output = at_dyad(store, types);
    node(store, types.tape.bracket_arg, types.tape.bracket_arg_leaf, &[bracket], output)
}

/// The kind of bracket `node` is, `scope` or `square_brackets`, written or wrapped by
/// `build_bracket_arg`; `None` for any other node.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn bracket_kind(types: &Core, node: DyadPtr) -> Option<DyadPtr> {
    let node = types.through(node);
    let bracket = if dyad::ty(node) == types.tape.bracket_arg {
        *(dyad::value(node) as *const DyadPtr)
    } else {
        node
    };
    is_bracket(types, bracket).then(|| dyad::ty(bracket))
}

/// The type of a line evaluated where the call runs, and how its value moves: every line a whole
/// value moves out of, but a type or a literal, which the callee reads where it stands.
///
/// # Safety
/// `line` must be a reduced dyad from the store.
unsafe fn evaluated_line(types: &Core, line: DyadPtr) -> Option<(DyadPtr, Carry)> {
    if super::is_comptime_arg(types, line) {
        return None;
    }
    let t = super::read::output_type(types, line);
    Some((t, super::read::carry_of(types, t)?))
}

/// A value whose place holds a node's address, a type, a `dyad` or a node a Logos `parse`
/// builds, stands in a bracket as that node.
///
/// # Safety
/// `t` must be a type node from the store.
unsafe fn is_node_value(types: &Core, t: DyadPtr) -> bool {
    matches!(super::read::place_layout(types, t), Some((super::read::Read::Container(_), _)))
}

/// A bracket handed to a placed call: each line is evaluated where the call runs, in its
/// frame, and the callee reads a new bracket of the same kind holding the values; a nested
/// bracket is evaluated here too, since the callee builds it where these names are not.
fn run_bracket_arg(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a `[bracket, op]` node `build_bracket_arg` built over a bracket node.
    unsafe {
        let bracket = rt.through(*(dyad::value(node) as *const DyadPtr));
        Ok(evaluate_bracket(rt, bracket)? as i64)
    }
}

/// # Safety
/// `bracket` must be a scope or `square_brackets` node from the store.
unsafe fn evaluate_bracket(rt: &mut Runtime, bracket: DyadPtr) -> Result<DyadPtr, RunError> {
    let lines = super::scope::exprs_of(bracket).unwrap_or(&[]);
    let mut bits = Vec::with_capacity(lines.len());
    for &line in lines {
        let line = rt.through(line);
        bits.push(match evaluated_line(rt.types(), line) {
            _ if is_bracket(rt.types(), line) => evaluate_bracket(rt, line)? as i64,
            Some((_, carry)) => super::assign::take(rt, carry, line)?,
            None => 0,
        });
    }
    let types: *const Core = rt.types();
    Ok(bracket_holding(rt.store(), &*types, bracket, &bits))
}

/// The lines with a value run in the compiled frame, their values in a block, a value moved by
/// its bytes as their address; only the new bracket, the store's, is made by calling back into
/// the seed, which copies those bytes while they live.
fn lower_bracket_arg(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a `[bracket, op]` node `build_bracket_arg` built over a bracket node.
    unsafe {
        let bracket = lw.through(*(dyad::value(node) as *const DyadPtr));
        lower_bracket(lw, bracket)
    }
}

/// # Safety
/// `bracket` must be a scope or `square_brackets` node from the store.
unsafe fn lower_bracket(lw: &mut Lowerer, bracket: DyadPtr) -> Result<Value, CompileError> {
    let lines = super::scope::exprs_of(bracket).unwrap_or(&[]);
    let mut bits = Vec::with_capacity(lines.len());
    for &line in lines {
        let line = lw.through(line);
        bits.push(match evaluated_line(lw.types(), line) {
            _ if is_bracket(lw.types(), line) => lower_bracket(lw, line)?,
            Some((_, carry @ Carry::Word(nt))) => {
                let v = super::assign::lower_take(lw, carry, line)?;
                lw.widen(v, nt)
            }
            Some((_, carry)) => super::assign::lower_take(lw, carry, line)?,
            None => lw.const_i64(0),
        });
    }
    let values = lw.spill(&bits);
    let bracket = lw.node_addr(bracket);
    let entry = compiled_bracket as *const () as usize;
    lw.call_seed(entry, &[bracket, values])
}

/// # Safety
/// Called only by compiled code, with its context, a bracket node from the store and a
/// block holding one container per line.
unsafe extern "C" fn compiled_bracket(
    ctx: *mut crate::run::Context,
    bracket: DyadPtr,
    values: *const i64,
) -> i64 {
    let rt = &mut *crate::run::runtime_of(ctx);
    let n = super::scope::exprs_of(bracket).map_or(0, <[DyadPtr]>::len);
    let bits = if n == 0 { &[][..] } else { std::slice::from_raw_parts(values, n) };
    let types: *const Core = rt.types();
    bracket_holding(rt.store(), &*types, bracket, bits) as i64
}

/// A new bracket of `bracket`'s kind whose lines are the values `bits` holds for its evaluated
/// lines: a node, and a nested bracket, as the node itself; a number or a pointer in a node of
/// its own type; a value moved by its bytes copied out of the address `bits` holds, standing in
/// its line's place as a value that ran. Its other lines stand as written.
///
/// # Safety
/// `bracket` must be a scope or `square_brackets` node from the store, `bits` one container
/// per line, each the address of a value's bytes where its line's value moves by them.
unsafe fn bracket_holding(
    store: &mut Store,
    types: &Core,
    bracket: DyadPtr,
    bits: &[i64],
) -> DyadPtr {
    let lines = super::scope::exprs_of(bracket).unwrap_or(&[]);
    let mut values = Vec::with_capacity(lines.len());
    for (&line, &b) in lines.iter().zip(bits) {
        values.push(match evaluated_line(types, line) {
            _ if is_bracket(types, types.through(line)) => b as usize as DyadPtr,
            Some((t, _)) if is_node_value(types, t) => b as usize as DyadPtr,
            Some((t, Carry::Word(nt))) => store.alloc_blob(t, &b.to_ne_bytes()[..nt.bytes()]),
            Some((t, Carry::Bytes(width))) => {
                let held =
                    store.alloc_blob(t, std::slice::from_raw_parts(b as usize as *const u8, width));
                super::ran::build(store, types, dyad::value(held) as i64, false, t)
            }
            None => line,
        });
    }
    let mut slots = [std::ptr::null_mut(); super::scope::SLOTS];
    slots[0] = super::array::build(store, types.array_, &values);
    store.alloc_words(dyad::ty(bracket), &slots)
}

fn run_cell_dyad_at(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an application built by this file's helpers; `tape_of` checks the handle.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
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
