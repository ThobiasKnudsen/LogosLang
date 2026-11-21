// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The value a type's bodies work on: a parse's node, `tape[0]`, or a `free`'s or `static`
//! function's value, bound as an unnamed parameter, a `dyad ?` place holding the value's
//! address: a node's, or a plain record's bytes. A field read, `tape[0].f` or bare `f`,
//! reaches field `f` of the value by its index: a node's slot holding the node the field
//! holds, or for a field of a number or pointer type the value that node yields; a plain
//! record's bytes at the field's offset. As `=`'s target, the write of the right side's
//! node, or of a node holding the value it yields. The owner type each read carries says
//! which. Only the per-run copy lowers.

use super::callable::{self, Callables};
use super::drop_model::{teardown_of_type, Teardown};
use super::read::{carry_of, place_layout, Carry, Read};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;
use cranelift_codegen::ir::Value;

/// The handles: the slot read and the slot write, each with its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct ThisIds {
    /// A field read: `[value, k, binding, fill, owner, op]`, yielding the node slot `k` holds;
    /// `binding` the field's, null for a read built at run, `fill` the mark below or null,
    /// `owner` the type whose value it is.
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    /// A field write: `[value, k, v, owner, binding, op]`, storing `v`'s node into slot `k`.
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    /// A read of a field typed by `build_field_read`: `[value, k, type, binding, fill, owner,
    /// op]`, the value it holds.
    pub load: DyadPtr,
    pub load_leaf: DyadPtr,
    /// The mark a field read carries when the type's own `parse` built it: the constructor's
    /// fill of its fresh node, which needs no `mut`.
    pub fill: DyadPtr,
    /// A write there: `[value, k, v, type, owner, binding, op]`, a node of `type` holding v's
    /// value.
    pub store: DyadPtr,
    pub store_leaf: DyadPtr,
    /// `[template, op]`: a new node of the template's type holding its slots, made per run.
    pub copy: DyadPtr,
    pub copy_leaf: DyadPtr,
    /// `[type, places, op]`: a new value of `type` holding a run's field values, made per run.
    pub pack: DyadPtr,
    pub pack_leaf: DyadPtr,
    /// `[read, op, output_type]`: `.type` of a `dyad` field of the parse's own node, the type of
    /// the node it holds, read as a tape cell's is.
    pub field_type: DyadPtr,
    pub field_type_leaf: DyadPtr,
    /// A field of the node `tape[0]` holds once the parse stamped a type it got as it ran:
    /// `[cell, name, scope, op, output_type]`, the field looked up by its name then and read as
    /// the gate read lets the parse's `scope` read it, yielding the node it holds.
    pub named: DyadPtr,
    pub named_leaf: DyadPtr,
    /// A write there: `[cell, name, scope, v, op]`.
    pub named_write: DyadPtr,
    pub named_write_leaf: DyadPtr,
    /// `[place, op, output_type]`: the address of the record a place keeps behind its pointer,
    /// for a `static` function that writes it, a new block where the place holds none.
    pub block: DyadPtr,
    pub block_leaf: DyadPtr,
}

/// Neither has a spelling: `.` builds the read right of a parse's `tape[0]`, a bare field
/// name in a `free` or `static` function builds it too, and `=` the write over it.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> ThisIds {
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
    let (slot, slot_leaf) =
        op(cx, &["value", "k", "binding", "fill", "owner", "op", "output_type"], run_slot);
    cx.lower.insert(slot, lower_slot);
    let (write, write_leaf) = op(cx, &["value", "k", "v", "owner", "binding", "op"], run_write);
    let (load, load_leaf) =
        op(cx, &["value", "k", "type", "binding", "fill", "owner", "op", "output_type"], run_load);
    cx.lower.insert(load, lower_load);
    let fill = {
        let record = meta::token_record(cx, meta::prec::INERT, crate::parse::Assoc::Left, &[]);
        cx.store.alloc_head(cx.type_, record)
    };
    let (store, store_leaf) =
        op(cx, &["value", "k", "v", "type", "owner", "binding", "op"], run_store);
    cx.lower.insert(store, lower_store);
    let (copy, copy_leaf) = op(cx, &["this", "op", "output_type"], run_copy);
    // SAFETY: `copy` carries the record `op` built.
    unsafe { meta::mark_own(copy) };
    cx.lower.insert(copy, lower_copy);
    let (pack, pack_leaf) = op(cx, &["type", "places", "op", "output_type"], run_pack);
    cx.lower.insert(pack, lower_pack);
    let (field_type, field_type_leaf) =
        op(cx, &["read", "op", "output_type"], super::tape::run_line_type);
    let (named, named_leaf) = op(cx, &["cell", "name", "scope", "op", "output_type"], run_named);
    let (named_write, named_write_leaf) =
        op(cx, &["cell", "name", "scope", "v", "op"], run_named_write);
    let (block, block_leaf) = op(cx, &["place", "op", "output_type"], run_block);
    cx.lower.insert(block, lower_block);
    ThisIds {
        slot,
        slot_leaf,
        write,
        write_leaf,
        load,
        load_leaf,
        fill,
        store,
        store_leaf,
        copy,
        copy_leaf,
        pack,
        pack_leaf,
        field_type,
        field_type_leaf,
        named,
        named_leaf,
        named_write,
        named_write_leaf,
        block,
        block_leaf,
    }
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

/// A write gives its line nothing, so its node has no output word.
fn act(store: &mut Store, op: DyadPtr, leaf: DyadPtr, operands: &[DyadPtr]) -> DyadPtr {
    let mut v = operands.to_vec();
    v.push(leaf);
    store.alloc_words(op, &v)
}

/// `k` is the field's index among the instance fields, as a `u64` literal; `owner` the type
/// whose value `this` is. The read gives the field's declared type, as the field read through
/// a name does; `@dyad` only for a field no type was declared for.
#[allow(clippy::too_many_arguments)]
fn build_slot(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    declared: DyadPtr,
    binding: DyadPtr,
    fill: DyadPtr,
    owner: DyadPtr,
) -> DyadPtr {
    let output = if declared.is_null() { super::pointer::at_dyad(store, types) } else { declared };
    let ops = [this, k, binding, fill, owner];
    node(store, types.this.slot, types.this.slot_leaf, &ops, output)
}

/// A type's own `parse` placing a call on its fresh node: each run of the call takes its own
/// instance, begun as the parse left the node.
///
/// # Safety
/// `template` must be a node from the store.
pub(crate) unsafe fn build_copy(store: &mut Store, types: &Core, template: DyadPtr) -> DyadPtr {
    node(store, types.this.copy, types.this.copy_leaf, &[template], dyad::ty(template))
}

/// A node of `ty` with each field at its default or null, then the words `run_body::TAIL_LEN`
/// counts.
///
/// # Safety
/// `ty` must be a record type from the store.
pub(crate) unsafe fn empty_node(store: &mut Store, ty: DyadPtr) -> DyadPtr {
    let mut slots: Vec<DyadPtr> = super::array::items(meta::record_fields_of(ty))
        .iter()
        .map(|&field| super::hole::default_in(field))
        .collect();
    slots.resize(slots.len() + super::run_body::TAIL_LEN, std::ptr::null_mut());
    store.alloc_words(ty, &slots)
}

/// Whether a field of `ty` holds a node itself, a `type`, a `dyad` or text, as its value.
///
/// # Safety
/// `ty` must be null or a type node from the store.
unsafe fn holds_node(types: &Core, ty: DyadPtr) -> bool {
    matches!(place_layout(types, ty), Some((Read::Container(t), _)) if !meta::is_node_valued(t, types.fn_type))
}

/// Whether a read of a field declared `declared` moves a whole value of that type: a number, a
/// pointer, a rational or a record, or a node a Logos `parse` built (DESIGN ›A value of a type
/// built by a Logos `parse` travels as a pointer‹); not a `?`, `type` or `dyad` field, whose read
/// is the node it holds.
///
/// # Safety
/// `declared` must be null or a type node from the store.
pub(crate) unsafe fn reads_whole(types: &Core, declared: DyadPtr) -> bool {
    !declared.is_null() && !holds_node(types, declared) && carry_of(types, declared).is_some()
}

/// A field read: a `load` when [`reads_whole`], so the read carries the declared type; else the
/// slot's node.
///
/// # Safety
/// `declared` must be null or a type node from the store.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn build_field_read(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    declared: DyadPtr,
    binding: DyadPtr,
    fill: DyadPtr,
    owner: DyadPtr,
) -> DyadPtr {
    if reads_whole(types, declared) {
        build_load(store, types, this, k, declared, binding, fill, owner)
    } else {
        build_slot(store, types, this, k, declared, binding, fill, owner)
    }
}

/// `ty` is the field's declared type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_load(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    ty: DyadPtr,
    binding: DyadPtr,
    fill: DyadPtr,
    owner: DyadPtr,
) -> DyadPtr {
    node(store, types.this.load, types.this.load_leaf, &[this, k, ty, binding, fill, owner], ty)
}

/// Either field read.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_field_read(types: &Core, node: DyadPtr) -> bool {
    dyad::ty(node) == types.this.slot || dyad::ty(node) == types.this.load
}

/// The binding of the field a read reaches, null for a read built at run.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn field_binding_of(types: &Core, read: DyadPtr) -> DyadPtr {
    let ops = dyad::value(read) as *const DyadPtr;
    *ops.add(if dyad::ty(read) == types.this.slot { 2 } else { 3 })
}

/// Whether the type's own `parse` built the read.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn is_fill(types: &Core, read: DyadPtr) -> bool {
    let ops = dyad::value(read) as *const DyadPtr;
    *ops.add(if dyad::ty(read) == types.this.slot { 3 } else { 4 }) == types.this.fill
}

/// The field's declared type, for a read of a number or pointer field.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn load_type(types: &Core, read: DyadPtr) -> Option<DyadPtr> {
    (dyad::ty(read) == types.this.load).then(|| *(dyad::value(read) as *const DyadPtr).add(2))
}

/// The type whose value the read reaches into.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn owner_of(types: &Core, read: DyadPtr) -> DyadPtr {
    let ops = dyad::value(read) as *const DyadPtr;
    *ops.add(if dyad::ty(read) == types.this.slot { 4 } else { 5 })
}

/// `.type` of `read`, a `dyad` field of the parse's own node.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn build_field_type(store: &mut Store, types: &Core, read: DyadPtr) -> DyadPtr {
    node(store, types.this.field_type, types.this.field_type_leaf, &[read], types.type_)
}

/// The field a read or a write of a field reaches, keyed as a check narrows a tape cell: the
/// value, the field's index, the whole field.
///
/// # Safety
/// `node` must be a node `is_field_read` accepts or a write `build_write` built.
pub(crate) unsafe fn field_key(
    types: &Core,
    node: DyadPtr,
) -> Option<(DyadPtr, i32, super::tape::Line)> {
    let ops = dyad::value(node) as *const DyadPtr;
    let k = types.through(*ops.add(1));
    let k = super::numtype::read_scalar(dyad::ty(k), dyad::value(k) as *const u8);
    Some((types.through(*ops), i32::try_from(k).ok()?, super::tape::Line::Cell))
}

/// What a place is as a field path (DESIGN ›`move` and `free` take a field path too‹).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldPath {
    /// No field: a name, a cell, a value.
    NoField,
    /// A field reached from a name through fields: the name's binding, then each field's
    /// binding, the name's first.
    From(DyadPtr, Vec<DyadPtr>),
    /// A field reached through a pointer, an element or a value no name holds.
    Unfollowed,
}

/// What `node` reads a field of, with the field's binding, null for a read built at run: a field
/// read or a record's `field` place; `None` for any other node, a binding's own field (`x:f`)
/// included.
///
/// # Safety
/// `node` must be a reduced dyad from the store.
pub(crate) unsafe fn field_step(types: &Core, node: DyadPtr) -> Option<(DyadPtr, DyadPtr)> {
    if is_field_read(types, node) {
        Some((*(dyad::value(node) as *const DyadPtr), field_binding_of(types, node)))
    } else if dyad::ty(node) == types.field_ && types.binding_field_of(node).is_none() {
        Some(super::instance::field_parts(node))
    } else {
        None
    }
}

/// Whether `read` reaches its value through names alone: each field read or record field steps
/// to what it reads into, down to a name or the parameter holding the value a body is about, so
/// running `read` runs nothing but reads.
///
/// # Safety
/// `read` must be a reduced dyad from the store.
pub(crate) unsafe fn reached_by_name(types: &Core, read: DyadPtr) -> bool {
    let mut at = read;
    while !at.is_null() {
        match field_step(types, at) {
            Some((into, _)) => at = into,
            None => return at != read && dyad::ty(at) == types.binding_,
        }
    }
    false
}

/// The field path `node` is, read off the reads it is built of: each field read or record
/// field steps to the value it reads into, until a name, the value `held` the parse read the
/// name `root` as, or something else.
///
/// # Safety
/// `node` must be a reduced dyad from the store; `root` null or a binding dyad.
pub(crate) unsafe fn field_path(
    types: &Core,
    node: DyadPtr,
    root: DyadPtr,
    held: DyadPtr,
) -> FieldPath {
    let mut fields = Vec::new();
    let mut at = node;
    while let Some((into, field)) = field_step(types, at) {
        fields.push(field);
        at = into;
    }
    if dyad::ty(at) == types.deref_ && super::pointer::reads_folded_field(types, at) {
        return FieldPath::Unfollowed;
    }
    if fields.is_empty() {
        return FieldPath::NoField;
    }
    if !root.is_null() && !at.is_null() && (at == held || at == types.through(root)) {
        at = root;
    }
    if at.is_null() || dyad::ty(at) != types.binding_ || fields.iter().any(|f| f.is_null()) {
        return FieldPath::Unfollowed;
    }
    fields.reverse();
    FieldPath::From(at, fields)
}

/// The parse's own node keeps the node placing makes of the right side, landed in the field's
/// type when it gives a value (›A field is filled at run, per evaluation; its type decides how
/// the operand is used‹ [[R193]]). Elsewhere a number or pointer field takes the value the right
/// side yields, molded to the field's type, a node-valued field the node it yields, and any
/// other field the node placing makes of it.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts; `value` a reduced dyad.
pub(crate) unsafe fn build_write(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    let ops = dyad::value(read) as *const DyadPtr;
    let (this, k, owner) = (*ops, *ops.add(1), owner_of(types, read));
    let binding = field_binding_of(types, read);
    let fill = is_fill(types, read);
    let write = |store: &mut Store, value: DyadPtr| {
        let value = if fill { super::place::build(store, types, value) } else { value };
        act(store, types.this.write, types.this.write_leaf, &[this, k, value, owner, binding])
    };
    if let Some(ty) = load_type(types, read) {
        if meta::is_node_valued(ty, types.fn_type) {
            let value = super::land(store, types, value, ty)?;
            return Ok(write(store, value));
        }
        // An `if` whose arms give two types gives a value in each, which lands on its own.
        let arms_differ =
            super::read::value_type(types, value) == Err(crate::parse::ParseError::ArmsDiffer);
        let yields_value = matches!(super::operand_of(types, value), super::Operand::Literal)
            || arms_differ
            || match place_layout(types, super::read::output_type(types, value)) {
                Some((Read::Scalar(_) | Read::Rational | Read::Aggregate | Read::Behind(_), _)) => {
                    true
                }
                Some((Read::Pointer(p), _)) => p != types.dyad_,
                _ => false,
            };
        if yields_value {
            let value = super::land(store, types, value, ty)?;
            if fill {
                return Ok(write(store, value));
            }
            let ops = [this, k, value, ty, owner, binding];
            return Ok(act(store, types.this.store, types.this.store_leaf, &ops));
        }
    }
    super::read::value_type(types, value)?;
    let value = super::place::build(store, types, value);
    Ok(act(store, types.this.write, types.this.write_leaf, &[this, k, value, owner, binding]))
}

/// `tape[0].f` in a parse that stamps a type it gets as it runs: `f` is looked up among that
/// type's fields when the read runs, and a type without it is the error that says so (›`x.type`
/// reads the type word of the node a name or an expression stands for‹ [[R279]]), as is a field
/// the gate read does not let `scope`, where the read is written, reach.
pub(crate) fn build_named(
    store: &mut Store,
    types: &Core,
    cell: DyadPtr,
    name: &str,
    scope: DyadPtr,
) -> DyadPtr {
    let name = super::string::build_text(store, types.string_, name.as_bytes());
    let output = super::pointer::at_dyad(store, types);
    node(store, types.this.named, types.this.named_leaf, &[cell, name, scope], output)
}

/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_named(types: &Core, node: DyadPtr) -> bool {
    dyad::ty(node) == types.this.named
}

/// The right side is placed when the write runs and lands in the field's type there, as
/// [`build_write`] lands it where the field is known (›A field is filled at run, per evaluation;
/// its type decides how the operand is used‹ [[R193]]).
///
/// # Safety
/// `read` must be a node [`is_named`] accepts; `value` a reduced dyad.
pub(crate) unsafe fn build_named_write(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    match super::read::value_type(types, value) {
        Ok(_) | Err(crate::parse::ParseError::ArmsDiffer) => {}
        Err(e) => return Err(e),
    }
    let ops = dyad::value(read) as *const DyadPtr;
    let value = super::place::build(store, types, value);
    let ops = [*ops, *ops.add(1), *ops.add(2), value];
    Ok(act(store, types.this.named_write, types.this.named_write_leaf, &ops))
}

/// The index among `ty`'s fields of the one spelled `name` and its binding, resolved in the
/// type's own scope as a parse's field is resolved where the type is known.
///
/// # Safety
/// `ty` must be a record type from the store.
unsafe fn field_named(rt: &Runtime, ty: DyadPtr, name: &str) -> Option<(usize, DyadPtr)> {
    let mut scope = crate::parse::ScopeStack::new();
    scope.push(meta::record_scope_of(ty));
    let found = scope.resolve(rt.index()?, name).ok().filter(|r| r.matched == name.len())?;
    let fields = super::array::items(meta::record_fields_of(ty));
    Some((fields.iter().position(|&f| f == found.identity)?, found.binding))
}

/// The slot of the field a named read or write reaches, with the field's declared type; only the
/// node the parse is building has its fields reached so.
///
/// # Safety
/// `ops` must be the operands of a node [`build_named`] or [`build_named_write`] built.
unsafe fn named_slot(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<(*mut DyadPtr, usize, DyadPtr), RunError> {
    let this = rt.run(*ops)? as DyadPtr;
    if this.is_null() || rt.fresh_this() != Some(this) || rt.unstamped(this) {
        return Err(RunError::FieldBeforeStamp);
    }
    let ty = dyad::ty(this);
    let name = std::str::from_utf8(super::string::text(*ops.add(1))).unwrap_or("");
    let Some((k, binding)) = field_named(rt, ty, name) else {
        return Err(RunError::NoFieldNamed(Box::new(name.to_string())));
    };
    if !binding.is_null() && !super::gate::grants_read(rt.types(), binding, *ops.add(2)) {
        let e = crate::parse::ParseError::NotPub(Box::new(name.to_string()));
        return Err(RunError::Parse(Box::new(e)));
    }
    let field = super::array::items(meta::record_fields_of(ty))[k];
    Ok(((dyad::value(this) as *mut DyadPtr).add(k), k, super::hole::type_in(field)))
}

fn run_named(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a named read from `build_named`.
    unsafe {
        let (slot, k, _) = named_slot(rt, dyad::value(node) as *const DyadPtr)?;
        let held = slot.read_unaligned();
        if held.is_null() {
            return Err(RunError::UnfilledField(k));
        }
        Ok(held as i64)
    }
}

fn run_named_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a named write from `build_named_write`, its value operand a place node.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let placed = rt.run(*ops.add(3))? as DyadPtr;
        let (slot, _, declared) = named_slot(rt, ops)?;
        let types: *const Core = rt.types();
        let value = if reads_whole(&*types, declared) {
            super::land(rt.store(), &*types, placed, declared)
                .map_err(|e| RunError::Parse(Box::new(e)))?
        } else {
            placed
        };
        slot.write_unaligned(value);
        Ok(0)
    }
}

/// Where field `k` of the value lies: a node's slot, or the bytes of a plain record, the one a
/// `static` function called through it writes (DESIGN ›There is no `this`‹), with the field's
/// declared type; each with the field's index.
enum Field {
    Slot(*mut DyadPtr, usize),
    Bytes(*mut u8, DyadPtr, usize),
}

/// A plain record, laid out in bytes: neither a node a Logos `parse` builds nor one that runs.
///
/// # Safety
/// `owner` must be a type node from the store.
pub(crate) unsafe fn is_plain(types: &Core, owner: DyadPtr) -> bool {
    meta::is_record_type(owner)
        && meta::run_body_of(owner).is_null()
        && !meta::is_node_valued(owner, types.fn_type)
}

/// A plain record kept in its place's own bytes: one whose type fills no `free`, a `{ … }`'s
/// value included. One whose type fills `free` lives behind a pointer of its own (›`free` and
/// `move` end a name; no drop flag‹ [[R60]]).
///
/// # Safety
/// `t` must be null or a type node from the store.
pub(crate) unsafe fn in_bytes(types: &Core, t: DyadPtr) -> bool {
    is_plain(types, t) && !matches!(teardown_of_type(types, t), Teardown::Behind(_))
}

/// # Safety
/// `ops` must be the operands of a field read or write, `owner` its owner type and `name` its
/// field's binding, null for one built at run.
unsafe fn field_of(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<Field, RunError> {
    let this = receiver(rt, *ops, owner)?;
    field_at(rt, this, ops, owner, name)
}

/// Where a place of a record kept behind its pointer keeps the record's address, 0 while it holds
/// none (›`free` and `move` end a name; no drop flag‹ [[R60]]), with the record's type; `None` for
/// any other value `this` gives.
///
/// # Safety
/// `this` must be a reduced dyad from the store and `owner` a type node.
unsafe fn behind_place(
    rt: &mut Runtime,
    this: DyadPtr,
    owner: DyadPtr,
) -> Result<Option<(*mut usize, DyadPtr)>, RunError> {
    let this = rt.through(this);
    let Teardown::Behind(t) = teardown_of_type(rt.types(), owner) else {
        return Ok(None);
    };
    if super::read::place_of(rt.types(), this).is_none() {
        return Ok(None);
    }
    super::pointer::place_addr(rt, this).map(|slot| Some((slot.cast(), t)))
}

/// [`field_of`] for a write: a place of a record kept behind its pointer that holds none yet takes
/// a new block first, its other fields zero, so `mut a := bag ?` holds 0 until a write gives it a
/// value.
///
/// # Safety
/// As [`field_of`].
unsafe fn field_to_write(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<Field, RunError> {
    if let Some((slot, t)) = behind_place(rt, *ops, owner)? {
        if slot.read_unaligned() == 0 {
            let block = super::drop_model::new_behind(rt, t)?;
            slot.write_unaligned(block as usize);
        }
    }
    field_of(rt, ops, owner, name)
}

/// [`field_of`], `None` where a place of a record kept behind its pointer holds none: a field of
/// a record that is not there holds nothing to free.
///
/// # Safety
/// As [`field_of`].
unsafe fn field_if_held(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<Option<Field>, RunError> {
    if let Some((slot, _)) = behind_place(rt, *ops, owner)? {
        if slot.read_unaligned() == 0 {
            return Ok(None);
        }
    }
    field_of(rt, ops, owner, name).map(Some)
}

/// The value a field read or write of `owner` works on, what its first operand `this` gives: a
/// plain record's bytes by their address, as every whole read of a record hands them, so a record
/// that lies in a node's own bytes is read where it lies; any other value as it runs.
///
/// # Safety
/// `this` must be a reduced dyad from the store and `owner` a type node.
unsafe fn receiver(rt: &mut Runtime, this: DyadPtr, owner: DyadPtr) -> Result<DyadPtr, RunError> {
    if is_plain(rt.types(), owner) {
        return super::by_copy::record_addr(rt, this).map(<*mut u8>::cast);
    }
    Ok(rt.run(this)? as DyadPtr)
}

/// [`receiver`]'s compiled half.
///
/// # Safety
/// As [`receiver`].
unsafe fn lower_receiver(
    lw: &mut Lowerer,
    this: DyadPtr,
    owner: DyadPtr,
) -> Result<Value, CompileError> {
    if is_plain(lw.types(), owner) {
        return super::by_copy::lower_record_addr(lw, this);
    }
    lw.lower(this)
}

/// As [`field_of`], of the value `this` the read's first operand gave.
///
/// # Safety
/// As [`field_of`]; `this` is null or what the first operand yields.
unsafe fn field_at(
    rt: &mut Runtime,
    this: DyadPtr,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<Field, RunError> {
    if this.is_null() {
        return own_name(rt, ops, name);
    }
    if is_plain(rt.types(), owner) {
        let bytes = this as *mut u8;
        let k = rt.run(*ops.add(1))?;
        let (index, (field, offset)) = usize::try_from(k)
            .ok()
            .and_then(|k| Some((k, super::instance::field_offsets(owner).nth(k)?)))
            .ok_or(RunError::BadIndex(k))?;
        return Ok(Field::Bytes(bytes.add(offset), super::hole::type_in(field), index));
    }
    if rt.unstamped(this) {
        return Err(RunError::FieldBeforeStamp);
    }
    let slots = dyad::value(this) as *mut DyadPtr;
    let k = rt.run(*ops.add(1))?;
    if k < 0 {
        return Err(RunError::BadIndex(k));
    }
    Ok(Field::Slot(slots.add(k as usize), k as usize))
}

/// A field of no value: a bare call made in a `{ … }`'s own lines hands on none, being about the
/// scope it builds, whose field `name` is that name itself, where it lies (›`{ … }` outside text
/// is a scope kept as a value, read with `.`‹ [[R374]]). Any other field of no value is the
/// checked error.
///
/// # Safety
/// As [`field_of`].
unsafe fn own_name(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    name: DyadPtr,
) -> Result<Field, RunError> {
    if name.is_null() || !rt.types().is_storage(name) {
        return Err(RunError::NoThis);
    }
    let ty = rt.types().type_of(name);
    let addr = rt.place_addr(name).ok_or(RunError::NoActivation)?;
    let k = rt.run(*ops.add(1))?;
    let index = usize::try_from(k).map_err(|_| RunError::BadIndex(k))?;
    Ok(Field::Bytes(addr, ty, index))
}

/// Where the node a field a write reaches holds is kept: a node's slot, or the word a plain
/// record keeps for a field whose value is a node's address, a `type`, a `dyad` or a node a Logos
/// `parse` built, packed so at any offset; any other field of a plain record holds no node.
///
/// # Safety
/// As `field_of`.
unsafe fn slot_of(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<(*mut DyadPtr, usize), RunError> {
    let field = field_to_write(rt, ops, owner, name)?;
    slot_in(rt, field)
}

/// As [`slot_of`], of the value `this` the read's first operand gave.
///
/// # Safety
/// As [`field_at`].
unsafe fn slot_at(
    rt: &mut Runtime,
    this: DyadPtr,
    ops: *const DyadPtr,
    owner: DyadPtr,
    name: DyadPtr,
) -> Result<(*mut DyadPtr, usize), RunError> {
    let field = field_at(rt, this, ops, owner, name)?;
    slot_in(rt, field)
}

/// # Safety
/// `field` must lie in a live value.
unsafe fn slot_in(rt: &Runtime, field: Field) -> Result<(*mut DyadPtr, usize), RunError> {
    match field {
        Field::Slot(slot, k) => Ok((slot, k)),
        Field::Bytes(addr, ty, k)
            if matches!(place_layout(rt.types(), ty), Some((Read::Container(_), _))) =>
        {
            Ok((addr.cast(), k))
        }
        Field::Bytes(..) => Err(RunError::NoThis),
    }
}

/// Where a pointer field `read` reaches keeps its eight bytes: a plain record's own bytes, or
/// the value of the node a slot holds; `None` while the slot is unwritten or the record is not
/// there.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts, of a pointer field.
pub(crate) unsafe fn pointer_bytes(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<Option<*mut u8>, RunError> {
    let (owner, name) = (owner_of(rt.types(), read), field_binding_of(rt.types(), read));
    Ok(match field_if_held(rt, dyad::value(read) as *const DyadPtr, owner, name)? {
        None => None,
        Some(Field::Bytes(bytes, ..)) => Some(bytes),
        Some(Field::Slot(slot, _)) => (!(*slot).is_null()).then(|| dyad::value(*slot)),
    })
}

/// Where the field `read` reaches lies: a node's slot, the node pointer it holds, or a plain
/// record's bytes.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn field_addr(rt: &mut Runtime, read: DyadPtr) -> Result<*mut u8, RunError> {
    let (owner, name) = (owner_of(rt.types(), read), field_binding_of(rt.types(), read));
    Ok(match field_of(rt, dyad::value(read) as *const DyadPtr, owner, name)? {
        Field::Slot(slot, _) => slot as *mut u8,
        Field::Bytes(bytes, ..) => bytes,
    })
}

/// [`field_addr`], `None` where the record the field lies in is not there: its place holds none.
///
/// # Safety
/// As [`field_addr`].
pub(crate) unsafe fn held_field_addr(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<Option<*mut u8>, RunError> {
    let (owner, name) = (owner_of(rt.types(), read), field_binding_of(rt.types(), read));
    Ok(field_if_held(rt, dyad::value(read) as *const DyadPtr, owner, name)?.map(|f| match f {
        Field::Slot(slot, _) => slot as *mut u8,
        Field::Bytes(bytes, ..) => bytes,
    }))
}

/// The node the field a read reaches holds; an unfilled one is the checked error.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn held(rt: &mut Runtime, read: DyadPtr) -> Result<DyadPtr, RunError> {
    let owner = owner_of(rt.types(), read);
    let this = receiver(rt, *(dyad::value(read) as *const DyadPtr), owner)?;
    held_at(rt, read, this)
}

/// As [`held`], of the value `this` the read's first operand gave.
///
/// # Safety
/// As [`held`]; `this` is null or what the read's first operand yields.
unsafe fn held_at(rt: &mut Runtime, read: DyadPtr, this: DyadPtr) -> Result<DyadPtr, RunError> {
    let (owner, name) = (owner_of(rt.types(), read), field_binding_of(rt.types(), read));
    let (slot, k) = slot_at(rt, this, dyad::value(read) as *const DyadPtr, owner, name)?;
    let held = slot.read_unaligned();
    if held.is_null() {
        return Err(RunError::UnfilledField(k));
    }
    Ok(held)
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a slot node from the store; its value holds a node with a slot per field.
    unsafe { Ok(held(rt, node)? as i64) }
}

/// The receiver is read in the compiled frame and the node by [`held_at`], as [`lower_load`]
/// reads a value.
fn lower_slot(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a slot node from `build_slot`, its receiver its first operand.
    unsafe {
        let owner = owner_of(lw.types(), node);
        let this = lower_receiver(lw, *(dyad::value(node) as *const DyadPtr), owner)?;
        let read = lw.node_addr(node);
        lw.call_seed(compiled_slot as *const () as usize, &[read, this])
    }
}

/// # Safety
/// Called only by compiled code, with its context, a slot node `lower_slot` baked and the
/// value its first operand gave there.
unsafe extern "C" fn compiled_slot(ctx: *mut crate::run::Context, read: DyadPtr, this: i64) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        held_at(rt, read, this as usize as DyadPtr).map(|n| n as i64)
    })
}

fn run_load(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a load node from `build_load`, its receiver its first operand.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let this = receiver(rt, *ops, *ops.add(5))?;
        load(rt, node, this)
    }
}

/// The value the field holds, as a whole read of a place of its type hands it over: read from a
/// plain record's bytes, or yielded by the node a slot holds; an unfilled slot is the checked
/// error. A number field of the parse's own node has a value only where it holds a constant, and
/// a plain number a number field holds reads as that number type, exactly (›A tape read checked
/// against a number type reads as that number‹ [[R229]]).
///
/// # Safety
/// `node` must be a load node from `build_load`; `this` what its first operand yields. The node
/// a slot holds is one this file or a constructor stored; a record's bytes are its layout's.
unsafe fn load(rt: &mut Runtime, node: DyadPtr, this: DyadPtr) -> Result<i64, RunError> {
    let ops = dyad::value(node) as *const DyadPtr;
    let ty = *ops.add(2);
    let carry = carry_of(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
    match field_at(rt, this, ops, *ops.add(5), *ops.add(3))? {
        Field::Bytes(addr, ..) => Ok(super::assign::load(carry, addr)),
        Field::Slot(slot, k) if (*slot).is_null() => Err(RunError::UnfilledField(k)),
        Field::Slot(slot, _)
            if super::numtype::is_scalar_type(ty)
                && (is_fill(rt.types(), node)
                    || super::read::output_type(rt.types(), *slot) == rt.types().rational) =>
        {
            super::tape::constant_value(rt.types(), *slot, ty)
        }
        Field::Slot(slot, _) => super::assign::take(rt, carry, *slot),
    }
}

/// The receiver is read in the compiled frame and the field by [`load`], so both tiers read a
/// field one way.
fn lower_load(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a load node from `build_load`, `[this, k, type, binding, fill, owner]`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let ty = *ops.add(2);
        // A rational or a record travels as the address of its bytes.
        let nt = match carry_of(lw.types(), ty) {
            Some(Carry::Word(nt)) => Some(nt),
            Some(Carry::Bytes(_)) => None,
            None => return Err(CompileError::NotLowerable(node)),
        };
        let this = lower_receiver(lw, *ops, *ops.add(5))?;
        let read = lw.node_addr(node);
        let bits = lw.call_seed(compiled_load as *const () as usize, &[read, this])?;
        Ok(match nt {
            Some(nt) => lw.narrow(bits, nt),
            None => bits,
        })
    }
}

/// # Safety
/// Called only by compiled code, with its context, a load node `lower_load` baked and the
/// value its first operand gave there.
unsafe extern "C" fn compiled_load(ctx: *mut crate::run::Context, read: DyadPtr, this: i64) -> i64 {
    crate::run::interpret_step(ctx, |rt| load(rt, read, this as usize as DyadPtr))
}

/// Into a plain record's bytes, the value itself. Into a slot, a number or pointer as a fresh
/// node of the field's type holding it, the way a literal holds its own; a value moved by its
/// bytes as the right side's node, which a read of the field takes it from. The right side runs
/// before the field is reached (›`=` sits beside `:=`, and returns nothing‹ [[R277]]), so one that
/// replaces the node a `dyad` the path goes through holds is written into the node held then.
fn run_store(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_load`; the type operand is a type node a whole value of which moves.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (value, ty, owner) = (*ops.add(2), *ops.add(3), *ops.add(4));
        let carry = carry_of(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
        // A value moved by its bytes into a node's slot is its node, which runs nothing here.
        let taken = match carry {
            Carry::Bytes(_) if !is_plain(rt.types(), owner) => None,
            _ => Some(super::assign::take(rt, carry, value)?),
        };
        match (field_to_write(rt, ops, owner, *ops.add(5))?, taken) {
            (Field::Bytes(addr, ..), Some(taken)) => super::assign::put(carry, addr, taken),
            (Field::Bytes(addr, ..), None) => {
                super::assign::put(carry, addr, super::assign::take(rt, carry, value)?);
            }
            (Field::Slot(slot, _), Some(bits)) if matches!(carry, Carry::Word(_)) => {
                *slot = rt.store().alloc_blob(ty, &bits.to_ne_bytes()[..carry.width()]);
            }
            (Field::Slot(slot, _), _) => *slot = value,
        }
        Ok(0)
    }
}

/// The right side runs first, then the field is reached in a plain record's bytes as
/// [`run_store`] reaches it: a name of the frame holding no record behind its pointer takes a new
/// block first. A field of a node, or of a record behind a place no frame holds, keeps the
/// function interpreted.
fn lower_store(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a store node from `build_write`, `[this, k, value, type, owner, binding]`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (this, value, ty) = (*ops, *ops.add(2), *ops.add(3));
        let (owner, binding) = (*ops.add(4), *ops.add(5));
        if binding.is_null() || !is_plain(lw.types(), owner) {
            return Err(CompileError::NotLowerable(node));
        }
        let carry = carry_of(lw.types(), ty).ok_or(CompileError::NotLowerable(node))?;
        let taken = super::assign::lower_take(lw, carry, value)?;
        let base = match teardown_of_type(lw.types(), owner) {
            Teardown::Behind(t) => {
                let slot = lower_held_slot(lw, this)?;
                let t = lw.node_addr(t);
                lw.call_seed(compiled_held_block as *const () as usize, &[slot, t])?
            }
            _ => lower_receiver(lw, this, owner)?,
        };
        let offset = crate::binding::Binding::read(binding).offset as i64;
        super::assign::lower_put(lw, carry, base, offset, taken);
        Ok(lw.const_i32(0))
    }
}

/// The receiver of a `static` function that writes it, `place` a name of a record kept behind its
/// pointer: the function writes the record's fields, so a place that holds none takes a new block
/// first, as a field write into it does.
///
/// # Safety
/// `place` must be a reduced dyad from the store, a place [`receives_block`] accepts.
pub(crate) unsafe fn build_block(store: &mut Store, types: &Core, place: DyadPtr) -> DyadPtr {
    let output = super::read::output_type(types, place);
    node(store, types.this.block, types.this.block_leaf, &[place], output)
}

/// Whether `place` is a name, field or cell of a record kept behind its pointer, which a `static`
/// function writing it reaches through [`build_block`].
///
/// # Safety
/// `place` must be a reduced dyad from the store.
pub(crate) unsafe fn receives_block(types: &Core, place: DyadPtr) -> bool {
    let place = types.through(place);
    let ty = super::read::output_type(types, place);
    matches!(teardown_of_type(types, ty), Teardown::Behind(_))
        && super::read::place_of(types, place).is_some()
}

fn run_block(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a block node from `build_block`.
    unsafe {
        let place = *(dyad::value(node) as *const DyadPtr);
        let owner = super::read::output_type(rt.types(), place);
        let (slot, t) = behind_place(rt, place, owner)?.ok_or(RunError::NoThis)?;
        if slot.read_unaligned() == 0 {
            let block = super::drop_model::new_behind(rt, t)?;
            slot.write_unaligned(block as usize);
        }
        Ok(slot.read_unaligned() as i64)
    }
}

/// The place is reached as [`lower_held_slot`] reaches it.
fn lower_block(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a block node from `build_block`.
    unsafe {
        let place = *(dyad::value(node) as *const DyadPtr);
        let ty = super::read::output_type(lw.types(), place);
        let Teardown::Behind(t) = teardown_of_type(lw.types(), ty) else {
            return Err(CompileError::NotLowerable(node));
        };
        let slot = lower_held_slot(lw, place)?;
        let t = lw.node_addr(t);
        lw.call_seed(compiled_held_block as *const () as usize, &[slot, t])
    }
}

/// Where `place` keeps the word it holds, compiled, 0 where it is a field of a record behind its
/// pointer that is not there: a name of the frame, or a field of a plain record reached through
/// the place that keeps it. Any other place keeps the function interpreted.
///
/// # Safety
/// `place` must be a reduced dyad from the store.
pub(crate) unsafe fn lower_held_slot(
    lw: &mut Lowerer,
    place: DyadPtr,
) -> Result<Value, CompileError> {
    let place = lw.through(place);
    if lw.types().is_storage(place) {
        return lw.place_addr(place);
    }
    if dyad::ty(place) == lw.types().this.load {
        let ops = dyad::value(place) as *const DyadPtr;
        let (this, binding, owner) = (*ops, *ops.add(3), *ops.add(5));
        if !binding.is_null() && is_plain(lw.types(), owner) {
            let offset = crate::binding::Binding::read(binding).offset as i64;
            if in_bytes(lw.types(), owner) {
                let base = lower_receiver(lw, this, owner)?;
                return Ok(lw.offset_addr(base, offset));
            }
            let outer = lower_held_slot(lw, this)?;
            let offset = lw.const_i64(offset);
            return lw.call_seed(compiled_field_slot as *const () as usize, &[outer, offset]);
        }
    }
    Err(CompileError::NotLowerable(place))
}

/// # Safety
/// Called only by compiled code, with `outer` 0 or the address of a place holding a record behind
/// its pointer or 0, and `offset` a field's offset in that record.
unsafe extern "C" fn compiled_field_slot(
    _ctx: *mut crate::run::Context,
    outer: *const usize,
    offset: i64,
) -> i64 {
    if outer.is_null() {
        return 0;
    }
    match outer.read_unaligned() {
        0 => 0,
        block => block as i64 + offset,
    }
}

/// `slot` 0, a field of a record that is not there, is the fault a read through that record gives.
///
/// # Safety
/// Called only by compiled code, with its context, 0 or the address of a place holding a record of
/// `t` behind its pointer or 0, and `t` a type [`teardown_of_type`] answers [`Teardown::Behind`].
unsafe extern "C" fn compiled_held_block(
    ctx: *mut crate::run::Context,
    slot: *mut usize,
    t: DyadPtr,
) -> i64 {
    crate::run::interpret_step(ctx, |rt| {
        if slot.is_null() {
            return Err(RunError::NullPointer);
        }
        // SAFETY: the caller's contract.
        unsafe {
            if slot.read_unaligned() == 0 {
                let block = super::drop_model::new_behind(rt, t)?;
                slot.write_unaligned(block as usize);
            }
            Ok(slot.read_unaligned() as i64)
        }
    })
}

/// The value written is the right side's node, the operand as graph, never a number read out of
/// it, run before the field is reached, as [`run_store`] runs it.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the value operand is a reduced dyad.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let value = rt.run(*ops.add(2))? as DyadPtr;
        let (slot, _) = slot_of(rt, ops, *ops.add(3), *ops.add(4))?;
        slot.write_unaligned(value);
        Ok(0)
    }
}

fn run_copy(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a copy node from `build_copy`.
    unsafe { Ok(copy_of(rt.store(), *(dyad::value(node) as *const DyadPtr)) as i64) }
}

/// The template is baked; the new node is the store's, so the step calls back into the seed.
fn lower_copy(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a copy node from `build_copy`.
    let template = unsafe { *(dyad::value(node) as *const DyadPtr) };
    let template = lw.node_addr(template);
    lw.call_seed(compiled_copy as *const () as usize, &[template])
}

/// # Safety
/// Called only by compiled code, with its context and a template `build_copy` was handed.
unsafe extern "C" fn compiled_copy(ctx: *mut crate::run::Context, template: DyadPtr) -> i64 {
    copy_of((*crate::run::runtime_of(ctx)).store(), template) as i64
}

/// The slots are copied, never the nodes they hold: a field write replaces its slot.
///
/// # Safety
/// `template` must be a node `run_logos_ctor` minted, `[field…]` and the words
/// `run_body::TAIL_LEN` counts.
pub(crate) unsafe fn copy_of(store: &mut Store, template: DyadPtr) -> DyadPtr {
    let ty = dyad::ty(template);
    let n = super::array::items(meta::record_fields_of(ty)).len() + super::run_body::TAIL_LEN;
    let slots = std::slice::from_raw_parts(dyad::value(template) as *const DyadPtr, n).to_vec();
    store.alloc_words(ty, &slots)
}

/// The value a bare `static` call from a `run` hands on: a new value of `ty` whose fields are
/// the run's own, `places` its parameters in field order (DESIGN ›There is no `this`‹). The
/// node itself would not do: its fields hold the operands as written, which the run evaluates.
pub(crate) fn build_pack(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    places: &[DyadPtr],
) -> DyadPtr {
    let places = super::array::build(store, types.array_, places);
    node(store, types.this.pack, types.this.pack_leaf, &[ty, places], ty)
}

/// A number or pointer is held as a node of its type, as a field write stores it; any other
/// value is the address the slot holds.
///
/// # Safety
/// `ty` must be a record type from the store; `places` its run's parameters in field order,
/// `bits` their values.
unsafe fn pack_of(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    places: &[DyadPtr],
    bits: &[i64],
) -> DyadPtr {
    use super::read::{place_layout, Read};
    let mut slots: Vec<DyadPtr> = places
        .iter()
        .zip(bits)
        .map(|(&place, &b)| {
            let pty = types.type_of(place);
            match (!pty.is_null()).then(|| place_layout(types, pty)).flatten() {
                Some((Read::Scalar(_) | Read::Pointer(_), width)) => {
                    store.alloc_blob(pty, &b.to_ne_bytes()[..width])
                }
                _ => b as DyadPtr,
            }
        })
        .collect();
    slots.resize(slots.len() + super::run_body::TAIL_LEN, std::ptr::null_mut());
    store.alloc_words(ty, &slots)
}

fn run_pack(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a pack node from `build_pack`, `[type, places, op]`.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (ty, places) = (*ops, super::array::items(*ops.add(1)));
        let bits = places.iter().map(|&p| rt.run(p)).collect::<Result<Vec<_>, _>>()?;
        let types: *const Core = rt.types();
        Ok(pack_of(rt.store(), &*types, ty, places, &bits) as i64)
    }
}

/// Each place is read at its own width and travels widened, as an argument does.
fn lower_pack(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    use cranelift_codegen::ir::types;
    // SAFETY: `node` is a pack node from `build_pack`; its places are the run's frame places.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (ty, arr) = (*ops, *ops.add(1));
        let mut values = Vec::new();
        for &place in super::array::items(arr) {
            let pty = lw.types().type_of(place);
            values.push(if super::numtype::is_scalar_type(pty) {
                let nt = super::numtype::of_type_node(pty);
                let v = lw.read_place(place, nt.cranelift_type())?;
                lw.widen(v, nt)
            } else {
                lw.read_place(place, types::I64)?
            });
        }
        let argv = lw.spill(&values);
        let (ty, arr) = (lw.node_addr(ty), lw.node_addr(arr));
        lw.call_seed(compiled_pack as *const () as usize, &[ty, arr, argv])
    }
}

/// # Safety
/// Called only by compiled code, with its context, the operands `lower_pack` baked and
/// `argv` one value per place.
unsafe extern "C" fn compiled_pack(
    ctx: *mut crate::run::Context,
    ty: DyadPtr,
    places: DyadPtr,
    argv: *const i64,
) -> i64 {
    let rt = &mut *crate::run::runtime_of(ctx);
    let places = super::array::items(places);
    let bits = std::slice::from_raw_parts(argv, places.len());
    let types: *const Core = rt.types();
    pack_of(rt.store(), &*types, ty, places, bits) as i64
}
