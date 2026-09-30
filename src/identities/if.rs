// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `if cond then` with an optional `else else`, each branch a bracket or the
//! next expression: the node is `[cond, then, else, then_ends, else_ends]`, the else slot
//! null when absent, and each `_ends` the names that arm frees at its end because the
//! other arm moved or freed them (DESIGN ›`move` and `free` are static: the parse marks the
//! name dead‹, rule 2). The surface parse lives in [`crate::parse::Parser::parse_if`]; here
//! the identity, its run native and lowering, and the `else` token.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::Assoc;
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

const IF_COND: usize = 0;
const IF_THEN: usize = 1;
const IF_ELSE: usize = 2;
/// Null, or an `array` of the held places the arm frees at its end, in declaration order.
const IF_THEN_ENDS: usize = 3;
const IF_ELSE_ENDS: usize = 4;

/// Returns `(identity, leaf, else token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["condition", "then", "else", "then_ends", "else_ends", "op"],
    );
    let if_ = cx.store.alloc_head(cx.type_, record);
    cx.declare("if", if_);
    cx.metas.insert(if_, |p, id, tape| {
        let node = p.parse_if(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(if_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // `else` is a parse-only token between the branches, not a function.
    let record = meta::record(cx.store, meta::TOKEN_TAG, meta::prec::INERT);
    let else_ = cx.store.alloc_head(cx.type_, record);
    cx.declare("else", else_);

    (if_, leaf, else_)
}

/// `els` null for an else-less `if`; no arm frees anything yet.
pub(crate) fn build(
    store: &mut Store,
    types: &Core,
    cond: DyadPtr,
    then: DyadPtr,
    els: DyadPtr,
) -> DyadPtr {
    let none = std::ptr::null_mut();
    store.alloc_words(types.if_, &[cond, then, els, none, none, types.ops.if_])
}

/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// The places each arm frees at its end, the then arm's first; an else-less `if` gains the
/// arm that frees the second list.
///
/// # Safety
/// As [`branches`]; the lists' store outlives the slices.
pub(crate) unsafe fn arm_ends<'a>(node: DyadPtr) -> (&'a [DyadPtr], &'a [DyadPtr]) {
    let p = dyad::value(node) as *const DyadPtr;
    let list = |at: usize| {
        let arr = *p.add(at);
        if arr.is_null() {
            &[][..]
        } else {
            array::items(arr)
        }
    };
    (list(IF_THEN_ENDS), list(IF_ELSE_ENDS))
}

/// # Safety
/// `node` must be an `if` node [`build`] made, not yet run; `then_ends` and `else_ends` held
/// places.
pub(crate) unsafe fn set_arm_ends(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    then_ends: &[DyadPtr],
    else_ends: &[DyadPtr],
) {
    let p = dyad::value(node) as *mut DyadPtr;
    for (at, ends) in [(IF_THEN_ENDS, then_ends), (IF_ELSE_ENDS, else_ends)] {
        if !ends.is_empty() {
            *p.add(at) = array::build(store, types.array_, ends);
        }
    }
}

/// An else-less `if` yields unit either way, matching the compiled merge. The arm that ran
/// frees, at its end or on a `return` or fault through it, what the other arm moved or freed;
/// a fault in the arm is the error shown, whatever that cleanup meets.
fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `if` node [`build`] made; its ends are held places.
    unsafe {
        let (cond, then, els) = branches(node);
        let (then_ends, else_ends) = arm_ends(node);
        let (arm, ends) = if rt.run(cond)? != 0 { (then, then_ends) } else { (els, else_ends) };
        let value = if arm.is_null() { Ok(0) } else { rt.run(arm) };
        let ended = ends.iter().rev().try_for_each(|&place| super::drop_model::end_held(rt, place));
        match (value, ended) {
            (Err(fault), _) if !matches!(fault, RunError::Return(_)) => Err(fault),
            (_, Err(e)) => Err(e),
            (value, Ok(())) => {
                let value = value?;
                Ok(if els.is_null() { 0 } else { value })
            }
        }
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an `if` node [`build`] made.
    unsafe {
        let (cond, then, els) = branches(node);
        let (then_ends, else_ends) = arm_ends(node);
        if els.is_null() {
            return lw.lower_if_stmt(cond, then, then_ends, else_ends);
        }
        lw.lower_if(cond, [(then, then_ends), (els, else_ends)])
    }
}
