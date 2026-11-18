// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The value a type's bodies work on: a parse's node, `tape[0]`, or a `free`'s or `share`
//! function's value, bound as an unnamed parameter, a `dyad ?` place holding the value's
//! address: a node's, or a plain record's bytes. A field read, `tape[0].f` or bare `f`,
//! reaches field `f` of the value by its index: a node's slot holding the node the field
//! holds, or for a field of a number or pointer type the value that node yields; a plain
//! record's bytes at the field's offset. As `=`'s target, the write of the right side's
//! node, or of a node holding the value it yields. The owner type each read carries says
//! which. Only the per-run copy lowers.

use super::callable::{self, Callables};
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
    /// A field write: `[value, k, v, owner, op]`, storing `v`'s node into slot `k`.
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    /// A read of a field typed by `build_field_read`: `[value, k, type, binding, fill, owner,
    /// op]`, the value it holds.
    pub load: DyadPtr,
    pub load_leaf: DyadPtr,
    /// The mark a field read carries when the type's own `parse` built it: the constructor's
    /// fill of its fresh node, which `immut` alone vetoes.
    pub fill: DyadPtr,
    /// A write there: `[value, k, v, type, owner, op]`, a node of `type` holding v's value.
    pub store: DyadPtr,
    pub store_leaf: DyadPtr,
    /// `[template, op]`: a new node of the template's type holding its slots, made per run.
    pub copy: DyadPtr,
    pub copy_leaf: DyadPtr,
    /// `[type, places, op]`: a new value of `type` holding a run's field values, made per run.
    pub pack: DyadPtr,
    pub pack_leaf: DyadPtr,
    /// A parse's read of the `output_type` its node's mint keeps: `[value, op, output_type]`.
    pub mint_output: DyadPtr,
    pub mint_output_leaf: DyadPtr,
    /// A write there: `[value, v, op]`, choosing the node's mint from its fields' types.
    pub mint_output_write: DyadPtr,
    pub mint_output_write_leaf: DyadPtr,
    /// `[read, op, output_type]`: `.type` of a `?` field of the parse's own node, the type of
    /// the node it holds, read as a tape cell's is.
    pub field_type: DyadPtr,
    pub field_type_leaf: DyadPtr,
}

/// Neither has a spelling: `.` builds the read right of a parse's `tape[0]`, a bare field
/// name in a `free` or `share` function builds it too, and `=` the write over it.
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
    let (write, write_leaf) = op(cx, &["value", "k", "v", "owner", "op"], run_write);
    let (load, load_leaf) =
        op(cx, &["value", "k", "type", "binding", "fill", "owner", "op", "output_type"], run_load);
    cx.lower.insert(load, lower_load);
    let fill = {
        let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
        cx.store.alloc_head(cx.type_, record)
    };
    let (store, store_leaf) = op(cx, &["value", "k", "v", "type", "owner", "op"], run_store);
    let (copy, copy_leaf) = op(cx, &["this", "op", "output_type"], run_copy);
    cx.lower.insert(copy, lower_copy);
    let (pack, pack_leaf) = op(cx, &["type", "places", "op", "output_type"], run_pack);
    cx.lower.insert(pack, lower_pack);
    let (mint_output, mint_output_leaf) = op(cx, &["value", "op", "output_type"], run_mint_output);
    let (mint_output_write, mint_output_write_leaf) =
        op(cx, &["value", "v", "op"], run_mint_output_write);
    let (field_type, field_type_leaf) =
        op(cx, &["read", "op", "output_type"], super::tape::run_line_type);
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
        mint_output,
        mint_output_leaf,
        mint_output_write,
        mint_output_write_leaf,
        field_type,
        field_type_leaf,
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
/// whose value `this` is.
pub(crate) fn build_slot(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    binding: DyadPtr,
    fill: DyadPtr,
    owner: DyadPtr,
) -> DyadPtr {
    let output = super::pointer::at_dyad(store, types);
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

/// A node of `ty` with each field at its default or null, then the words `run_body::tail_len`
/// counts.
///
/// # Safety
/// `ty` must be a record type from the store.
pub(crate) unsafe fn empty_node(store: &mut Store, ty: DyadPtr) -> DyadPtr {
    let mut slots: Vec<DyadPtr> = super::array::items(meta::record_fields_of(ty))
        .iter()
        .map(|&field| super::hole::default_in(field))
        .collect();
    slots.resize(slots.len() + super::run_body::tail_len(ty), std::ptr::null_mut());
    store.alloc_words(ty, &slots)
}

/// `tape[0].output_type` in the parse of a type that mints: the member its node's mint keeps
/// (›A node's output type is written by its parse, on the mint for a type that mints‹ [[R194]]).
pub(crate) fn build_mint_output(store: &mut Store, types: &Core, this: DyadPtr) -> DyadPtr {
    node(store, types.this.mint_output, types.this.mint_output_leaf, &[this], types.type_)
}

/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_mint_output(types: &Core, node: DyadPtr) -> bool {
    dyad::ty(node) == types.this.mint_output
}

/// The parse's write of its node's mint's `output_type`, which chooses the mint (›The v0.1.0
/// demo: a user-defined power operator `^`‹ [[R366]]).
///
/// # Safety
/// `read` must be a node `is_mint_output` accepts; `value` a reduced dyad.
pub(crate) unsafe fn build_mint_output_write(
    store: &mut Store,
    types: &Core,
    read: DyadPtr,
    value: DyadPtr,
) -> Result<DyadPtr, crate::parse::ParseError> {
    super::read::value_type(types, value)?;
    // Unplaced: the mint keeps the type its right side gives while the parse runs, not a node.
    let this = *(dyad::value(read) as *const DyadPtr);
    let ops = [this, value];
    Ok(act(store, types.this.mint_output_write, types.this.mint_output_write_leaf, &ops))
}

/// The node a parse is building, checked as a field read checks it.
///
/// # Safety
/// `this` must be the value operand of a node `build_mint_output` or its write built.
unsafe fn parsed_node(rt: &mut Runtime, this: DyadPtr) -> Result<DyadPtr, RunError> {
    let node = rt.run(this)? as DyadPtr;
    if node.is_null() {
        return Err(RunError::NoThis);
    }
    if rt.unstamped(node) {
        return Err(RunError::FieldBeforeStamp);
    }
    Ok(node)
}

fn run_mint_output(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a read from `build_mint_output`; the node it reads is a record type's.
    unsafe {
        let this = parsed_node(rt, *(dyad::value(node) as *const DyadPtr))?;
        super::mint::shared_field(dyad::ty(this), super::mint::SHARED_FIELD)
            .map(|t| t as i64)
            .ok_or(RunError::MintUnchosen)
    }
}

/// The mint is chosen from the types of the fields the parse has written so far.
fn run_mint_output_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a write from `build_mint_output_write`, `[value, v, op]`; the node it
    // writes is one of a type `takes_mint` accepts, its fields the slots before its tail.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let this = parsed_node(rt, *ops)?;
        let output = rt.run(*ops.add(1))? as DyadPtr;
        let types: *const Core = rt.types();
        let origin = super::mint::origin(dyad::ty(this));
        let key = super::run_body::field_types(&*types, origin, this)
            .map_err(|e| RunError::Parse(Box::new(e)))?
            .ok_or(RunError::MintUnchosen)?;
        let mint = super::mint::stamp(rt.store(), &*types, this, &key);
        meta::install_mint_output(mint, output);
        Ok(0)
    }
}

/// Whether a field of `ty` holds a node itself, a `type` or a `dyad`, as its value.
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
        build_slot(store, types, this, k, binding, fill, owner)
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

/// `.type` of `read`, a `?` field of the parse's own node.
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
    loop {
        if is_field_read(types, at) {
            fields.push(field_binding_of(types, at));
            at = *(dyad::value(at) as *const DyadPtr);
        } else if dyad::ty(at) == types.field_ && types.binding_field_of(at).is_none() {
            let (record, field) = super::instance::field_parts(at);
            fields.push(field);
            at = record;
        } else if dyad::ty(at) == types.deref_ && super::pointer::reads_folded_field(types, at) {
            return FieldPath::Unfollowed;
        } else {
            break;
        }
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
    let fill = is_fill(types, read);
    let write = |store: &mut Store, value: DyadPtr| {
        let value = if fill { super::place::build(store, types, value) } else { value };
        act(store, types.this.write, types.this.write_leaf, &[this, k, value, owner])
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
                Some((Read::Scalar(_) | Read::Rational | Read::Aggregate, _)) => true,
                Some((Read::Pointer(p), _)) => p != types.dyad_,
                _ => false,
            };
        if yields_value {
            let value = super::land(store, types, value, ty)?;
            if fill {
                return Ok(write(store, value));
            }
            let ops = [this, k, value, ty, owner];
            return Ok(act(store, types.this.store, types.this.store_leaf, &ops));
        }
    }
    super::read::value_type(types, value)?;
    let value = super::place::build(store, types, value);
    Ok(act(store, types.this.write, types.this.write_leaf, &[this, k, value, owner]))
}

/// Where field `k` of the value lies: a node's slot, or the bytes of a plain record, the one a
/// `share` function called through it writes (DESIGN ›There is no `this`‹), with the field's
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

/// # Safety
/// `ops` must be the operands of a field read or write, `owner` its owner type.
unsafe fn field_of(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
) -> Result<Field, RunError> {
    let this = rt.run(*ops)? as DyadPtr;
    field_at(rt, this, ops, owner)
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
) -> Result<Field, RunError> {
    if this.is_null() {
        return Err(RunError::NoThis);
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

/// Where the node a field holds is kept: a node's slot, or the word a plain record keeps for
/// a `type` or `dyad` field, packed so at any offset; any other field of a plain record holds no
/// node.
///
/// # Safety
/// As `field_of`.
unsafe fn slot_of(
    rt: &mut Runtime,
    ops: *const DyadPtr,
    owner: DyadPtr,
) -> Result<(*mut DyadPtr, usize), RunError> {
    match field_of(rt, ops, owner)? {
        Field::Slot(slot, k) => Ok((slot, k)),
        Field::Bytes(addr, ty, k) if holds_node(rt.types(), ty) => Ok((addr.cast(), k)),
        Field::Bytes(..) => Err(RunError::NoThis),
    }
}

/// Where a pointer field `read` reaches keeps its eight bytes: a plain record's own bytes, or
/// the value of the node a slot holds; `None` while the slot is unwritten.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts, of a pointer field.
pub(crate) unsafe fn pointer_bytes(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<Option<*mut u8>, RunError> {
    let owner = owner_of(rt.types(), read);
    Ok(match field_of(rt, dyad::value(read) as *const DyadPtr, owner)? {
        Field::Bytes(bytes, ..) => Some(bytes),
        Field::Slot(slot, _) => (!(*slot).is_null()).then(|| dyad::value(*slot)),
    })
}

/// Where the field `read` reaches lies: a node's slot, the node pointer it holds, or a plain
/// record's bytes.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn field_addr(rt: &mut Runtime, read: DyadPtr) -> Result<*mut u8, RunError> {
    let owner = owner_of(rt.types(), read);
    Ok(match field_of(rt, dyad::value(read) as *const DyadPtr, owner)? {
        Field::Slot(slot, _) => slot as *mut u8,
        Field::Bytes(bytes, ..) => bytes,
    })
}

/// The node the field a read reaches holds; an unfilled one is the checked error.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn held(rt: &mut Runtime, read: DyadPtr) -> Result<DyadPtr, RunError> {
    let owner = owner_of(rt.types(), read);
    let (slot, k) = slot_of(rt, dyad::value(read) as *const DyadPtr, owner)?;
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

fn run_load(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a load node from `build_load`, its receiver its first operand.
    unsafe {
        let this = rt.run(*(dyad::value(node) as *const DyadPtr))? as DyadPtr;
        load(rt, node, this)
    }
}

/// The value the field holds, as a whole read of a place of its type hands it over: read from a
/// plain record's bytes, or yielded by the node a slot holds; an unfilled slot is the checked
/// error. A number field of the parse's own node has a value only where it holds a constant.
///
/// # Safety
/// `node` must be a load node from `build_load`; `this` what its first operand yields. The node
/// a slot holds is one this file or a constructor stored; a record's bytes are its layout's.
unsafe fn load(rt: &mut Runtime, node: DyadPtr, this: DyadPtr) -> Result<i64, RunError> {
    let ops = dyad::value(node) as *const DyadPtr;
    let ty = *ops.add(2);
    let carry = carry_of(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
    match field_at(rt, this, ops, *ops.add(5))? {
        Field::Bytes(addr, ..) => Ok(super::assign::load(carry, addr)),
        Field::Slot(slot, k) if (*slot).is_null() => Err(RunError::UnfilledField(k)),
        Field::Slot(slot, _) if is_fill(rt.types(), node) && super::numtype::is_scalar_type(ty) => {
            super::tape::constant_number(rt.types(), *slot, ty)
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
        let carry = carry_of(lw.types(), ty);
        // A rational travels as the address of its bytes; a record's bytes do not lower yet.
        let nt = match carry {
            Some(Carry::Word(nt)) => Some(nt),
            Some(Carry::Bytes(_)) if ty == lw.types().rational => None,
            _ => return Err(CompileError::NotLowerable(node)),
        };
        let this = lw.lower(*ops)?;
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
/// bytes as the right side's node, which a read of the field takes it from.
fn run_store(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_load`; the type operand is a type node a whole value of which moves.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (value, ty) = (*ops.add(2), *ops.add(3));
        let carry = carry_of(rt.types(), ty).ok_or(RunError::NoLayout(ty))?;
        match (field_of(rt, ops, *ops.add(4))?, carry) {
            (Field::Bytes(addr, ..), _) => {
                let taken = super::assign::take(rt, carry, value)?;
                super::assign::put(carry, addr, taken);
            }
            (Field::Slot(slot, _), Carry::Bytes(_)) => *slot = value,
            (Field::Slot(slot, _), Carry::Word(_)) => {
                let bits = rt.run(value)?;
                *slot = rt.store().alloc_blob(ty, &bits.to_ne_bytes()[..carry.width()]);
            }
        }
        Ok(0)
    }
}

/// The value written is the right side's node, the operand as graph, never a number read out of it.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the value operand is a reduced dyad.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (slot, _) = slot_of(rt, ops, *ops.add(3))?;
        let value = rt.run(*ops.add(2))? as DyadPtr;
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
/// `run_body::tail_len` counts.
unsafe fn copy_of(store: &mut Store, template: DyadPtr) -> DyadPtr {
    let ty = dyad::ty(template);
    let n = super::array::items(meta::record_fields_of(ty)).len() + super::run_body::tail_len(ty);
    let slots = std::slice::from_raw_parts(dyad::value(template) as *const DyadPtr, n).to_vec();
    store.alloc_words(ty, &slots)
}

/// The value a bare `share` call from a `run` hands on: a new value of `ty` whose fields are
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
    slots.resize(slots.len() + super::run_body::tail_len(ty), std::ptr::null_mut());
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
