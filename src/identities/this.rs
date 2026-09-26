// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! The value a type's bodies work on: a parse's node, `tape[0]`, or a `drop`'s or `share`
//! function's value, bound as an unnamed parameter, a `dyad ?` place holding the node's
//! address. A field read, `tape[0].f` or bare `f`, reaches the slot at
//! `f`'s index among the instance fields: read, the node it holds, or for a field of a
//! number or pointer type the value that node yields; as `=`'s target, the write of the
//! right side's node, or of a node holding the value it yields. A field of a type a Logos
//! `parse` builds holds that node, whose address is the value. Only the per-run copy lowers.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;
use cranelift_codegen::ir::Value;

/// The handles: the slot read and the slot write, each with its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct ThisIds {
    /// A field read: `[node, k, op]`, yielding the node slot `k` holds.
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    /// A field write: `[node, k, value, op]`, storing `v`'s node into slot `k`.
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    /// A read of a field typed by `build_field_read`: `[node, k, type, op]`, the value it holds.
    pub load: DyadPtr,
    pub load_leaf: DyadPtr,
    /// A write there: `[node, k, value, type, op]`, a node of `type` holding v's value.
    pub store: DyadPtr,
    pub store_leaf: DyadPtr,
    /// `[template, op]`: a new node of the template's type holding its slots, made per run.
    pub copy: DyadPtr,
    pub copy_leaf: DyadPtr,
    /// `[type, places, op]`: a new value of `type` holding a run's field values, made per run.
    pub pack: DyadPtr,
    pub pack_leaf: DyadPtr,
    /// `[type, record, op]`: a new value of `type` holding a copy of a plain record's fields.
    pub unpack: DyadPtr,
    pub unpack_leaf: DyadPtr,
}

/// Neither has a spelling: `.` builds the read right of a parse's `tape[0]`, a bare field
/// name in a `drop` or `share` function builds it too, and `=` the write over it.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> ThisIds {
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
    let (slot, slot_leaf) = op(cx, &["this", "k", "op"], run_slot);
    let (write, write_leaf) = op(cx, &["this", "k", "value", "op"], run_write);
    let (load, load_leaf) = op(cx, &["this", "k", "type", "op"], run_load);
    let (store, store_leaf) = op(cx, &["this", "k", "value", "type", "op"], run_store);
    let (copy, copy_leaf) = op(cx, &["this", "op"], run_copy);
    cx.lower.insert(copy, lower_copy);
    let (pack, pack_leaf) = op(cx, &["type", "places", "op"], run_pack);
    cx.lower.insert(pack, lower_pack);
    let (unpack, unpack_leaf) = op(cx, &["type", "record", "op"], run_unpack);
    cx.lower.insert(unpack, lower_unpack);
    ThisIds {
        slot,
        slot_leaf,
        write,
        write_leaf,
        load,
        load_leaf,
        store,
        store_leaf,
        copy,
        copy_leaf,
        pack,
        pack_leaf,
        unpack,
        unpack_leaf,
    }
}

fn node(store: &mut Store, op: DyadPtr, leaf: DyadPtr, operands: &[DyadPtr]) -> DyadPtr {
    let mut v = operands.to_vec();
    v.push(leaf);
    let value = store.alloc_operands(&v);
    store.alloc_raw(op, value)
}

/// `k` is the field's index among the instance fields, as a `u64` literal.
pub(crate) fn build_slot(store: &mut Store, types: &Core, this: DyadPtr, k: DyadPtr) -> DyadPtr {
    node(store, types.this.slot, types.this.slot_leaf, &[this, k])
}

/// A type's own `parse` placing a call on its fresh node: each run of the call takes its own
/// instance, begun as the parse left the node.
pub(crate) fn build_copy(store: &mut Store, types: &Core, template: DyadPtr) -> DyadPtr {
    node(store, types.this.copy, types.this.copy_leaf, &[template])
}

/// A node of `ty` with each field at its default or null, then the run's terminator and
/// the slot for the node's field-type set.
///
/// # Safety
/// `ty` must be a record type from the store.
pub(crate) unsafe fn empty_node(store: &mut Store, ty: DyadPtr) -> DyadPtr {
    let mut slots: Vec<DyadPtr> = super::array::items(meta::record_fields_of(ty))
        .iter()
        .map(|&field| if (*field).value.is_null() { std::ptr::null_mut() } else { field })
        .collect();
    slots.extend([std::ptr::null_mut(); 2]);
    let value = store.alloc_operands(&slots);
    store.alloc_raw(ty, value)
}

/// A field read: a `load` when the field's declared type says what it holds, a number, a
/// pointer, or a node a Logos `parse` built (DESIGN ›A value of a type built by a Logos
/// `parse` travels as a pointer‹), so the read carries that type; else the slot's node.
///
/// # Safety
/// `declared` must be null or a type node from the store.
pub(crate) unsafe fn build_field_read(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    declared: DyadPtr,
) -> DyadPtr {
    use super::read::{place_layout, Read};
    let typed = !declared.is_null()
        && match place_layout(types, declared) {
            Some((Read::Scalar(_) | Read::Pointer(_), _)) => true,
            Some((Read::Container(t), _)) => meta::is_node_valued(t, types.fn_type),
            _ => false,
        };
    if typed {
        build_load(store, types, this, k, declared)
    } else {
        build_slot(store, types, this, k)
    }
}

/// `ty` is the field's declared type.
pub(crate) fn build_load(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    node(store, types.this.load, types.this.load_leaf, &[this, k, ty])
}

/// Either field read.
///
/// # Safety
/// `node` must be a valid dyad from the store.
pub(crate) unsafe fn is_field_read(types: &Core, node: DyadPtr) -> bool {
    (*node).ty == types.this.slot || (*node).ty == types.this.load
}

/// The field's declared type, for a read of a number or pointer field.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts.
pub(crate) unsafe fn load_type(types: &Core, read: DyadPtr) -> Option<DyadPtr> {
    ((*read).ty == types.this.load).then(|| *((*read).value as *const DyadPtr).add(2))
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
    let ops = (*read).value as *const DyadPtr;
    let (this, k) = (*ops, *ops.add(1));
    if let Some(ty) = load_type(types, read) {
        // The slot holds the node the right side yields, its address, never the expression.
        if meta::is_node_valued(ty, types.fn_type) {
            if super::node_type_of(types, value) != Some(ty) {
                return Err(crate::parse::ParseError::TypeMismatch);
            }
            return Ok(node(store, types.this.write, types.this.write_leaf, &[this, k, value]));
        }
        let yields_value = match super::numtype_of(types, value) {
            super::Operand::Concrete(_) | super::Operand::Literal => true,
            super::Operand::Pointer(p) => p != types.dyad_,
            super::Operand::NonNumeric => false,
        };
        if yields_value {
            let value = super::commit_fn_body(store, types, value, ty)?;
            let fits = match (super::read::place_layout(types, ty), super::numtype_of(types, value))
            {
                (Some((super::read::Read::Scalar(nt), _)), super::Operand::Concrete(v)) => nt == v,
                (Some((super::read::Read::Pointer(p), _)), super::Operand::Pointer(v)) => p == v,
                _ => false,
            };
            if !fits {
                return Err(crate::parse::ParseError::TypeMismatch);
            }
            return Ok(node(store, types.this.store, types.this.store_leaf, &[this, k, value, ty]));
        }
    }
    let value = super::tape::cell_arg(store, types, value);
    Ok(node(store, types.this.write, types.this.write_leaf, &[this, k, value]))
}

unsafe fn slot_of(
    rt: &mut Runtime,
    ops: *const DyadPtr,
) -> Result<(*mut DyadPtr, usize), RunError> {
    let this = rt.run(*ops)? as DyadPtr;
    if this.is_null() {
        return Err(RunError::NoThis);
    }
    if rt.unstamped(this) {
        return Err(RunError::FieldBeforeStamp);
    }
    let slots = (*this).value as *mut DyadPtr;
    if slots.is_null() {
        return Err(RunError::NoThis);
    }
    let k = rt.run(*ops.add(1))?;
    if k < 0 {
        return Err(RunError::BadIndex(k));
    }
    Ok((slots.add(k as usize), k as usize))
}

/// The node the slot `read` reaches holds, `None` while the slot is unwritten.
///
/// # Safety
/// `read` must be a node `is_field_read` accepts, over a value that holds a node.
pub(crate) unsafe fn field_node(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<Option<DyadPtr>, RunError> {
    let (slot, _) = slot_of(rt, (*read).value as *const DyadPtr)?;
    Ok((!(*slot).is_null()).then_some(*slot))
}

/// The slot `read` reaches, as the address of the node pointer it holds.
///
/// # Safety
/// As `field_node`.
pub(crate) unsafe fn slot_addr(rt: &mut Runtime, read: DyadPtr) -> Result<*mut u8, RunError> {
    Ok(slot_of(rt, (*read).value as *const DyadPtr)?.0 as *mut u8)
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a slot node from the store; its value holds a node with a slot per field.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (slot, k) = slot_of(rt, ops)?;
        if (*slot).is_null() {
            return Err(RunError::UnfilledField(k));
        }
        Ok(*slot as i64)
    }
}

/// The node the slot holds yields the value; an unfilled slot is the checked error.
fn run_load(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the node a slot holds is one this file or a constructor stored.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (slot, k) = slot_of(rt, ops)?;
        if (*slot).is_null() {
            return Err(RunError::UnfilledField(k));
        }
        rt.run(*slot)
    }
}

/// A fresh node of the field's type, holding the value, the way a literal holds its own.
fn run_store(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the type operand is a number or pointer type node.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (slot, _) = slot_of(rt, ops)?;
        let bits = rt.run(*ops.add(2))?;
        let ty = *ops.add(3);
        let width = super::read::place_layout(rt.types(), ty).map_or(8, |(_, w)| w);
        let store = rt.store();
        let storage = store.alloc_bytes(&bits.to_ne_bytes()[..width]);
        *slot = store.alloc_raw(ty, storage);
        Ok(0)
    }
}

/// The value written is the right side's node, the operand as graph, never a number read out of it.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as `run_slot`; the value operand is a reduced dyad.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let (slot, _) = slot_of(rt, ops)?;
        let value = rt.run(*ops.add(2))? as DyadPtr;
        *slot = value;
        Ok(0)
    }
}

fn run_copy(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a copy node from `build_copy`.
    unsafe { Ok(copy_of(rt.store(), *((*node).value as *const DyadPtr)) as i64) }
}

/// The template is baked; the new node is the store's, so the step calls back into the seed.
fn lower_copy(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is a copy node from `build_copy`.
    let template = unsafe { *((*node).value as *const DyadPtr) };
    let template = lw.const_i64(template as i64);
    Ok(lw.call_seed(compiled_copy as *const () as usize, &[template]))
}

/// # Safety
/// Called only by compiled code, with a template `build_copy` was handed.
unsafe extern "C" fn compiled_copy(template: DyadPtr) -> i64 {
    copy_of((*crate::run::standing_by()).store(), template) as i64
}

/// The slots are copied, never the nodes they hold: a field write replaces its slot.
///
/// # Safety
/// `template` must be a node `run_logos_ctor` minted, `[field…, null, spec]`.
unsafe fn copy_of(store: &mut Store, template: DyadPtr) -> DyadPtr {
    let ty = (*template).ty;
    let n = super::array::items(meta::record_fields_of(ty)).len() + 2;
    let slots = std::slice::from_raw_parts((*template).value as *const DyadPtr, n).to_vec();
    let value = store.alloc_operands(&slots);
    store.alloc_raw(ty, value)
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
    node(store, types.this.pack, types.this.pack_leaf, &[ty, places])
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
            let pty = (*place).ty;
            match (!pty.is_null()).then(|| place_layout(types, pty)).flatten() {
                Some((Read::Scalar(_) | Read::Pointer(_), width)) => {
                    let storage = store.alloc_bytes(&b.to_ne_bytes()[..width]);
                    store.alloc_raw(pty, storage)
                }
                _ => b as DyadPtr,
            }
        })
        .collect();
    slots.extend([std::ptr::null_mut(); 2]);
    let value = store.alloc_operands(&slots);
    store.alloc_raw(ty, value)
}

fn run_pack(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a pack node from `build_pack`, `[type, places, op]`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
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
        let ops = (*node).value as *const DyadPtr;
        let (ty, arr) = (*ops, *ops.add(1));
        let mut values = Vec::new();
        for &place in super::array::items(arr) {
            let pty = (*place).ty;
            values.push(if super::numtype::is_scalar_type(pty) {
                let nt = super::numtype::of_type_node(pty);
                let v = lw.read_place(place, nt.cranelift_type())?;
                lw.widen(v, nt)
            } else {
                lw.read_place(place, types::I64)?
            });
        }
        let argv = lw.spill(&values);
        let (ty, arr) = (lw.const_i64(ty as i64), lw.const_i64(arr as i64));
        Ok(lw.call_seed(compiled_pack as *const () as usize, &[ty, arr, argv]))
    }
}

/// # Safety
/// Called only by compiled code, with the operands `lower_pack` baked and `argv` one value
/// per place.
unsafe extern "C" fn compiled_pack(ty: DyadPtr, places: DyadPtr, argv: *const i64) -> i64 {
    let rt = &mut *crate::run::standing_by();
    let places = super::array::items(places);
    let bits = std::slice::from_raw_parts(argv, places.len());
    let types: *const Core = rt.types();
    pack_of(rt.store(), &*types, ty, places, bits) as i64
}

/// The value a `share` function called through a plain record takes: a new value of `ty`
/// holding a copy of the record's fields, as a record crosses any call by copy (DESIGN ›There
/// is no `this`‹). `record` yields the record's bytes.
///
/// # Safety
/// `ty` must be a record type from the store whose `instance::layout` succeeds.
pub(crate) unsafe fn build_unpack(
    store: &mut Store,
    types: &Core,
    ty: DyadPtr,
    record: DyadPtr,
) -> DyadPtr {
    node(store, types.this.unpack, types.this.unpack_leaf, &[ty, record])
}

/// Each field is held as a node of its number type, as a field write stores it.
///
/// # Safety
/// `ty` must be a type `build_unpack` was handed; `bytes` its value's bytes.
unsafe fn unpack_of(store: &mut Store, ty: DyadPtr, bytes: *const u8) -> DyadPtr {
    let (fields, _) = super::instance::layout(ty).expect("`build_unpack`'s caller checked it");
    let mut slots: Vec<DyadPtr> = fields
        .iter()
        .map(|&(field, nt, offset)| {
            let storage =
                store.alloc_bytes(std::slice::from_raw_parts(bytes.add(offset), nt.bytes()));
            store.alloc_raw((*field).ty, storage)
        })
        .collect();
    slots.extend([std::ptr::null_mut(); 2]);
    let value = store.alloc_operands(&slots);
    store.alloc_raw(ty, value)
}

fn run_unpack(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an unpack node from `build_unpack`, `[type, record, op]`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let bytes = super::by_copy::record_addr(rt, *ops.add(1))?;
        Ok(unpack_of(rt.store(), *ops, bytes) as i64)
    }
}

fn lower_unpack(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: as `run_unpack`.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let bytes = super::by_copy::lower_record_addr(lw, *ops.add(1))?;
        let ty = lw.const_i64(*ops as i64);
        Ok(lw.call_seed(compiled_unpack as *const () as usize, &[ty, bytes]))
    }
}

/// # Safety
/// Called only by compiled code, with the type `lower_unpack` baked and its record's bytes.
unsafe extern "C" fn compiled_unpack(ty: DyadPtr, bytes: *const u8) -> i64 {
    let rt = &mut *crate::run::standing_by();
    unpack_of(rt.store(), ty, bytes) as i64
}
