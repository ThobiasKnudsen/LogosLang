// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! `if cond then` with an optional `else else`, each branch a bracket or the
//! next expression: the node is `[cond, then, else, then_ends, else_ends, condition_ends, op]`,
//! the else slot null when absent, each arm's `_ends` the names it frees at its end because
//! the other arm moved or freed them, and `condition_ends` both, freed by a condition that
//! leaves before either arm runs (DESIGN ›`move` and `free` are static: the parse marks the
//! name dead‹, rule 2). The surface parse lives in [`crate::parse::Parser::parse_if`]; here
//! the identity, its run native and lowering, and the `else` token.

use cranelift_codegen::ir::Value;

use super::callable::{self, Callables};
use super::{array, meta, Cx};
use crate::compile::{CompileError, Lowerer};
use crate::dyad;
use crate::dyad::DyadPtr;
use crate::parse::{Assoc, ParseError};
use crate::run::{RunError, Runtime};
use crate::store::Store;
use crate::Core;

const IF_COND: usize = 0;
const IF_THEN: usize = 1;
const IF_ELSE: usize = 2;
/// Null, or an `array` of held places in declaration order.
const IF_THEN_ENDS: usize = 3;
const IF_ELSE_ENDS: usize = 4;
const IF_CONDITION_ENDS: usize = 5;

/// Returns `(identity, leaf, else token)`.
pub(super) fn register(cx: &mut Cx, cs: &Callables) -> (DyadPtr, DyadPtr, DyadPtr) {
    let record = meta::operand_record(
        cx,
        meta::TUPLE_TAG,
        meta::prec::READER,
        Assoc::Left,
        &["condition", "then", "else", "then_ends", "else_ends", "condition_ends", "op"],
    );
    let if_ = cx.store.alloc_head(cx.type_, record);
    // SAFETY: `if_` carries the record built above, which names both arms.
    unsafe { meta::install_hands_on(if_, &["then", "else"], &[]) };
    cx.declare("if", if_);
    cx.metas.insert(if_, |p, id, tape| {
        let node = p.parse_if(id)?;
        tape.place(node);
        Ok(crate::parse::Constructed::Placed)
    });
    cx.lower.insert(if_, lower);
    let leaf = callable::mint_native(cx.store, cs.callable, run, cs.seed_native);

    // `else` is a parse-only token between the branches, not a function.
    let record = meta::token_record(cx, meta::prec::INERT, crate::parse::Assoc::Left, &[]);
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
    store.alloc_words(types.if_, &[cond, then, els, none, none, none, types.ops.if_])
}

/// What an `if` gives back where it is used as a value: what its arms hand on where every arm
/// gives a value of one type, else `void`. DESIGN ›`if` reads its own right side‹, ›A mint's
/// output type is written in its `static output_type` declaration; a type that does not mint has
/// its parse write it into each node‹ [[R194]].
///
/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn output_type(types: &Core, node: DyadPtr) -> DyadPtr {
    value_type(types, node).unwrap_or(types.void_)
}

/// The type an `if` gives where its value is used, or why it gives none: it needs an `else`,
/// every arm must give a value, the arms one type, and every arm make its value or every arm
/// borrow one. DESIGN ›`if` reads its own right side‹.
///
/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn value_type(types: &Core, node: DyadPtr) -> Result<DyadPtr, ParseError> {
    super::flow::value_type(types, node)
}

/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn branches(node: DyadPtr) -> (DyadPtr, DyadPtr, DyadPtr) {
    let p = dyad::value(node) as *const DyadPtr;
    (*p.add(IF_COND), *p.add(IF_THEN), *p.add(IF_ELSE))
}

/// The slots of the arms, the then arm's first; the else slot holds null for an else-less `if`.
///
/// # Safety
/// `node` must be an `if` node [`build`] made.
pub(crate) unsafe fn arm_slots(node: DyadPtr) -> (*mut DyadPtr, *mut DyadPtr) {
    let p = dyad::value(node) as *mut DyadPtr;
    (p.add(IF_THEN), p.add(IF_ELSE))
}

/// # Safety
/// As [`branches`]; the list's store outlives the slice.
unsafe fn ends_at<'a>(node: DyadPtr, at: usize) -> &'a [DyadPtr] {
    let arr = *(dyad::value(node) as *const DyadPtr).add(at);
    if arr.is_null() {
        &[]
    } else {
        array::items(arr)
    }
}

/// The places each arm frees at its end, the then arm's first; an else-less `if` gains the
/// arm that frees the second list.
///
/// # Safety
/// As [`ends_at`].
pub(crate) unsafe fn arm_ends<'a>(node: DyadPtr) -> (&'a [DyadPtr], &'a [DyadPtr]) {
    (ends_at(node, IF_THEN_ENDS), ends_at(node, IF_ELSE_ENDS))
}

/// # Safety
/// `node` must be an `if` node [`build`] made, not yet run; the ends held places.
pub(crate) unsafe fn set_ends(
    store: &mut Store,
    types: &Core,
    node: DyadPtr,
    [then_ends, else_ends, condition_ends]: [&[DyadPtr]; 3],
) {
    let p = dyad::value(node) as *mut DyadPtr;
    for (at, ends) in
        [(IF_THEN_ENDS, then_ends), (IF_ELSE_ENDS, else_ends), (IF_CONDITION_ENDS, condition_ends)]
    {
        if !ends.is_empty() {
            *p.add(at) = array::build(store, types.array_, ends);
        }
    }
}

fn run(rt: &mut Runtime, node: DyadPtr) -> Result<i64, RunError> {
    // SAFETY: `node` is an `if` node [`build`] made.
    unsafe { run_given(rt, node).map(|given| given.unwrap_or(0)) }
}

/// The arm the condition picks runs as a line, and what it gives the `if` gives (›A scope's
/// value is what it evaluates to, and `return` is an optional early exit from the enclosing
/// function‹ [[R207]]). The arm that ran frees, at its end or on a `return` or fault through it,
/// what the other arm moved or freed, and a condition that leaves frees what either arm would
/// have.
///
/// # Safety
/// `node` must be an `if` node [`build`] made; its ends held places.
pub(super) unsafe fn run_given(rt: &mut Runtime, node: DyadPtr) -> Result<Option<i64>, RunError> {
    let (cond, then, els) = branches(node);
    let (then_ends, else_ends) = arm_ends(node);
    let truth = match rt.run(cond) {
        Ok(truth) => truth,
        Err(left) => return end_after(rt, Err(left), ends_at(node, IF_CONDITION_ENDS)),
    };
    let (arm, ends) = if truth != 0 { (then, then_ends) } else { (els, else_ends) };
    let given = rt.run_line(arm);
    end_after(rt, given, ends)
}

/// Frees `ends`, last declared first, once `value` is known; a fault stays the error shown,
/// whatever that cleanup meets.
///
/// # Safety
/// `ends` must be held places laid out in the frame `rt` runs.
unsafe fn end_after<T>(
    rt: &mut Runtime,
    value: Result<T, RunError>,
    ends: &[DyadPtr],
) -> Result<T, RunError> {
    let ended = ends.iter().rev().try_for_each(|&place| super::drop_model::end_held(rt, place));
    match (value, ended) {
        (Err(fault), _) if !matches!(fault, RunError::Return(_)) => Err(fault),
        (_, Err(e)) => Err(e),
        (value, Ok(())) => value,
    }
}

fn lower(lw: &mut Lowerer, node: DyadPtr) -> Result<Value, CompileError> {
    // SAFETY: `node` is an `if` node [`build`] made.
    lw.lower_as_value(|lw| unsafe { lower_given(lw, node) })
}

/// [`run_given`]'s lowering: the value an arm gives passes out.
///
/// # Safety
/// `node` must be an `if` node [`build`] made; its ends held places.
pub(super) unsafe fn lower_given(lw: &mut Lowerer, node: DyadPtr) -> Result<(), CompileError> {
    let (cond, then, els) = branches(node);
    let (then_ends, else_ends) = arm_ends(node);
    let condition_ends = ends_at(node, IF_CONDITION_ENDS);
    lw.lower_if_given(cond, condition_ends, [(then, then_ends), (els, else_ends)])
}
