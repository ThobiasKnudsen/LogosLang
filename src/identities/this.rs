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
use super::numtype::{read_scalar_nt, write_scalar_nt, NumType};
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
    let fill = {
        let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
        cx.store.alloc_head(cx.type_, record)
    };
    let (store, store_leaf) = op(cx, &["value", "k", "v", "type", "owner", "op"], run_store);
    let (copy, copy_leaf) = op(cx, &["this", "op", "output_type"], run_copy);
    cx.lower.insert(copy, lower_copy);
    let (pack, pack_leaf) = op(cx, &["type", "places", "op", "output_type"], run_pack);
    cx.lower.insert(pack, lower_pack);
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
    // SAFETY: `dyad_` is a type node `Core::build` minted.
    let output = unsafe { super::pointer::make_pointer_type(store, types.type_, types.dyad_) };
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

/// A node of `ty` with each field at its default or null, then the run's terminator and
/// the slot for the node's field-type set.
///
/// # Safety
/// `ty` must be a record type from the store.
pub(crate) unsafe fn empty_node(store: &mut Store, ty: DyadPtr) -> DyadPtr {
    let mut slots: Vec<DyadPtr> = super::array::items(meta::record_fields_of(ty))
        .iter()
        .map(|&field| super::hole::default_in(field))
        .collect();
    slots.extend([std::ptr::null_mut(); 2]);
    store.alloc_words(ty, &slots)
}

/// A field read: a `load` when the field's declared type says what it holds, a number, a
/// pointer, or a node a Logos `parse` built (DESIGN ›A value of a type built by a Logos
/// `parse` travels as a pointer‹), so the read carries that type; else the slot's node.
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
    use super::read::{place_layout, Read};
    let typed = !declared.is_null()
        && match place_layout(types, declared) {
            Some((Read::Scalar(_) | Read::Pointer(_), _)) => true,
            Some((Read::Container(t), _)) => meta::is_node_valued(t, types.fn_type),
            _ => false,
        };
    if typed {
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

/// A number or pointer field takes the value the right side yields, molded to the
/// field's type; a right side that yields a node, the operand as graph, is stored as
/// that node (DESIGN ›Execution is function application‹, item 4).
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
    if let Some(ty) = load_type(types, read) {
        // The slot holds the node the right side yields, its address, never the expression.
        if meta::is_node_valued(ty, types.fn_type) {
            if super::node_output(types, value) != Some(ty) {
                return Err(crate::parse::ParseError::TypeMismatch);
            }
            let ops = [this, k, value, owner];
            return Ok(act(store, types.this.write, types.this.write_leaf, &ops));
        }
        let yields_value = matches!(super::operand_of(types, value), super::Operand::Literal)
            || match super::read::place_layout(types, super::read::output_type(types, value)) {
                Some((super::read::Read::Scalar(_), _)) => true,
                Some((super::read::Read::Pointer(p), _)) => p != types.dyad_,
                _ => false,
            };
        if yields_value {
            let value = super::commit_fn_body(store, types, value, ty)?;
            super::check_store_type(types, ty, value)?;
            let ops = [this, k, value, ty, owner];
            return Ok(act(store, types.this.store, types.this.store_leaf, &ops));
        }
    }
    let value = super::tape::cell_arg(store, types, value);
    let ops = [this, k, value, owner];
    Ok(act(store, types.this.write, types.this.write_leaf, &ops))
}

/// Where field `k` of the value lies: a node's slot, or the bytes of a plain record,
/// the one a `share` function called through it writes (DESIGN ›There is no `this`‹).
enum Field {
    Slot(*mut DyadPtr, usize),
    Bytes(*mut u8, NumType),
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
    if this.is_null() {
        return Err(RunError::NoThis);
    }
    if is_plain(rt.types(), owner) {
        let bytes = this as *mut u8;
        let k = rt.run(*ops.add(1))?;
        let (fields, _) = super::instance::layout(owner).map_err(|_| RunError::NoThis)?;
        let &(_, nt, offset) =
            usize::try_from(k).ok().and_then(|k| fields.get(k)).ok_or(RunError::BadIndex(k))?;
        return Ok(Field::Bytes(bytes.add(offset), nt));
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

/// A plain record's fields are numbers, never held as nodes.
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
        Field::Bytes(bytes, _) => Some(bytes),
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
        Field::Bytes(bytes, _) => bytes,
    })
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a slot node from the store; its value holds a node with a slot per field.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let (slot, k) = slot_of(rt, ops, *ops.add(4))?;
        if (*slot).is_null() {
            return Err(RunError::UnfilledField(k));
        }
        Ok(*slot as i64)
    }
}

/// The node the slot holds yields the value; an unfilled slot is the checked error.
fn run_load(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the node a slot holds is one this file or a constructor stored,
    // a record's bytes are its layout's.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        match field_of(rt, ops, *ops.add(5))? {
            Field::Bytes(addr, nt) => Ok(read_scalar_nt(nt, addr)),
            Field::Slot(slot, k) if (*slot).is_null() => Err(RunError::UnfilledField(k)),
            Field::Slot(slot, _) => rt.run(*slot),
        }
    }
}

/// A fresh node of the field's type, holding the value, the way a literal holds its own.
fn run_store(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_load`; the type operand is a number or pointer type node.
    unsafe {
        let ops = dyad::value(node) as *const DyadPtr;
        let field = field_of(rt, ops, *ops.add(4))?;
        let bits = rt.run(*ops.add(2))?;
        match field {
            Field::Bytes(addr, nt) => write_scalar_nt(nt, addr, bits),
            Field::Slot(slot, _) => {
                let ty = *ops.add(3);
                let width = super::read::place_layout(rt.types(), ty).map_or(8, |(_, w)| w);
                let store = rt.store();
                *slot = store.alloc_blob(ty, &bits.to_ne_bytes()[..width]);
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
        *slot = value;
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
    Ok(lw.call_seed(compiled_copy as *const () as usize, &[template]))
}

/// # Safety
/// Called only by compiled code, with its context and a template `build_copy` was handed.
unsafe extern "C" fn compiled_copy(ctx: *mut crate::run::Context, template: DyadPtr) -> i64 {
    copy_of((*crate::run::runtime_of(ctx)).store(), template) as i64
}

/// The slots are copied, never the nodes they hold: a field write replaces its slot.
///
/// # Safety
/// `template` must be a node `run_logos_ctor` minted, `[field…, null, spec]`.
unsafe fn copy_of(store: &mut Store, template: DyadPtr) -> DyadPtr {
    let ty = dyad::ty(template);
    let n = super::array::items(meta::record_fields_of(ty)).len() + 2;
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
    slots.extend([std::ptr::null_mut(); 2]);
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
        Ok(lw.call_seed(compiled_pack as *const () as usize, &[ty, arr, argv]))
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
