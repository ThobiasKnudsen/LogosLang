// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `this` inside a `parse` body or a fields-block `fn`: the node, bound as a hidden
//! parameter, a `dyad ?` place holding the node's address. `this.f` reaches the slot at
//! `f`'s index among the instance fields: read, the node it holds, or for a field of a
//! number or pointer type the value that node yields; as `=`'s target, the write of the
//! right side's node, or of a node holding the value it yields. Only the per-run copy lowers.

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
    /// `this.f` read: `[this, k, op]`, yielding the node slot `k` holds.
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    /// `this.f = v`: `[this, k, value, op]`, storing `v`'s node into slot `k`.
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
    /// `this.f` of a number or pointer field: `[this, k, type, op]`, the value it holds.
    pub load: DyadPtr,
    pub load_leaf: DyadPtr,
    /// `this.f = v` there: `[this, k, value, type, op]`, a node of `type` holding v's value.
    pub store: DyadPtr,
    pub store_leaf: DyadPtr,
    /// `[template, op]`: a new node of the template's type holding its slots, made per run.
    pub copy: DyadPtr,
    pub copy_leaf: DyadPtr,
}

/// Neither has a spelling: `.` builds the read when its left side is a parse body's
/// `this`, and `=` the write over it.
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

/// `ty` is the field's declared type, a number or pointer type.
pub(crate) fn build_load(
    store: &mut Store,
    types: &Core,
    this: DyadPtr,
    k: DyadPtr,
    ty: DyadPtr,
) -> DyadPtr {
    node(store, types.this.load, types.this.load_leaf, &[this, k, ty])
}

/// Either read `this.f` builds.
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
/// `read` must be a node `is_field_read` accepts, over a `this` that holds a node.
pub(crate) unsafe fn field_node(
    rt: &mut Runtime,
    read: DyadPtr,
) -> Result<Option<DyadPtr>, RunError> {
    let (slot, _) = slot_of(rt, (*read).value as *const DyadPtr)?;
    Ok((!(*slot).is_null()).then_some(*slot))
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a slot node from the store; `this` holds a node with a slot per field.
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
