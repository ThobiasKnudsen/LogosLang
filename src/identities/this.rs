// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `this` inside a `parse` body: the fresh node a constructor builds and places, bound
//! as the body's second hidden parameter, a `dyad ?` place holding the node's address.
//! `this.f` reaches the slot at `f`'s index among the instance fields: read, the node
//! it holds; as `=`'s target, the write of the right side's node. Nothing here lowers.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

/// The handles: the slot read and the slot write, each with its run leaf.
#[derive(Debug, Clone, Copy)]
pub struct ThisIds {
    /// `this.f` read: `[this, k, op]`, yielding the node slot `k` holds.
    pub slot: DyadPtr,
    pub slot_leaf: DyadPtr,
    /// `this.f = v`: `[this, k, value, op]`, storing `v`'s node into slot `k`.
    pub write: DyadPtr,
    pub write_leaf: DyadPtr,
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
    ThisIds { slot, slot_leaf, write, write_leaf }
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

/// # Safety
/// `slot` must be a node from `build_slot`.
pub(crate) unsafe fn build_write(
    store: &mut Store,
    types: &Core,
    slot: DyadPtr,
    value: DyadPtr,
) -> DyadPtr {
    let ops = (*slot).value as *const DyadPtr;
    let (this, k) = (*ops, *ops.add(1));
    let value = super::tape::cell_arg(store, types, value);
    node(store, types.this.write, types.this.write_leaf, &[this, k, value])
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
