// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `this` inside a `parse` body: the fresh node a constructor builds and
//! places (#133 slice 6; DESIGN ›Execution is function application‹,
//! amended 18 September 2026: "`this` in `parse` is a fresh node of the type
//! being defined … so its `value` block stands instantiated with every hole
//! its own, never tied to a cell; the constructor fills it by name, `this.lhs
//! = tape[-1]`, and places it with the tape's ordinary write, `tape[0] =
//! this`"; "`this` and `tape` are names the slot's type declares into the
//! body it reads, bound per run").
//!
//! The seed binds `this` as the parse body's second hidden parameter, a
//! `dyad ?` place holding the node's address, and mints the node per run of
//! the constructor ([`crate::parse::Parser::run_logos_ctor`]): `{type: T,
//! value: [slot per instance field …, null]}`, the operand record a node of
//! `T` runs as (DESIGN, 16 September 2026: "the operands of a node are its
//! instance fields … the constructor connects each operand to a field by
//! name"). `this.f` resolves `f` among the fields the `instance = (…)` block
//! above declared and reaches the slot at that index: read, the node the
//! slot holds; as `=`'s target, the write of the right side's node
//! ([`build_write`]) — the operand's graph, the first of a field's two
//! moments (DESIGN, 20 September 2026: "the operand slot graph is written
//! into at parse and the frame place it is evaluated into at run"). The
//! natives run interpreted, inside a constructor; nothing here lowers.

use super::callable::{self, Callables};
use super::{meta, Cx};
use crate::dyad::DyadPtr;
use crate::parse::CoreTypes;
use crate::run::{RunError, Runtime};
use crate::store::Store;

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

/// Register the two natives. Neither has a spelling: `.` builds the read
/// when its left side is a parse body's `this`, and `=` the write over it.
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

/// `this.f`: the read of slot `k` of the node `this` holds — `this` the
/// body's hidden parameter, `k` the field's index among the instance fields
/// as a `u64` literal.
pub(crate) fn build_slot(
    store: &mut Store,
    types: &CoreTypes,
    this: DyadPtr,
    k: DyadPtr,
) -> DyadPtr {
    node(store, types.this.slot, types.this.slot_leaf, &[this, k])
}

/// `this.f = v`: the write behind a slot target.
///
/// # Safety
/// `slot` must be a node from [`build_slot`].
pub(crate) unsafe fn build_write(
    store: &mut Store,
    types: &CoreTypes,
    slot: DyadPtr,
    value: DyadPtr,
) -> DyadPtr {
    let ops = (*slot).value as *const DyadPtr;
    let (this, k) = (*ops, *ops.add(1));
    node(store, types.this.write, types.this.write_leaf, &[this, k, value])
}

/// The slot `k` of the node the `this` operand holds: the operand run's
/// address plus `k`, or the checked error when `this` holds nothing.
unsafe fn slot_of(rt: &mut Runtime, ops: *const DyadPtr) -> Result<*mut DyadPtr, RunError> {
    let this = rt.run(*ops)? as DyadPtr;
    if this.is_null() {
        return Err(RunError::BadValue);
    }
    let slots = (*this).value as *mut DyadPtr;
    if slots.is_null() {
        return Err(RunError::BadValue);
    }
    let k = rt.run(*ops.add(1))?;
    if k < 0 {
        return Err(RunError::BadValue);
    }
    Ok(slots.add(k as usize))
}

fn run_slot(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is a slot node from the store; its operands are reduced
    // dyads, and `this` holds a node whose operand run has a slot per field.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let slot = slot_of(rt, ops)?;
        Ok(*slot as i64)
    }
}

/// The value written is the right side's *node*: what a tape read yields (a
/// cell's dyad), what a cell-type read yields (a type), what an identity
/// stands for — the operand as graph, never a number read out of it.
fn run_write(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: as [`run_slot`]; the value operand is a reduced dyad.
    unsafe {
        let ops = (*node).value as *const DyadPtr;
        let slot = slot_of(rt, ops)?;
        let value = rt.run(*ops.add(2))? as DyadPtr;
        *slot = value;
        Ok(0)
    }
}
